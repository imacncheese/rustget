use std::time::Duration;

use rustget_core::EngineConfig;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{NetError, Result};
use crate::probe::{FileProbe, Validator, parse_content_range};

/// Caller-supplied HTTP customisation shared by probe and transfer requests.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestOptions {
    pub headers: Vec<(String, String)>,
    pub basic_auth: Option<(String, String)>,
    pub bearer_token: Option<String>,
    pub cookie: Option<String>,
    pub proxy: Option<String>,
    /// Development only; never persisted by the CLI.
    pub accept_invalid_certs: bool,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct NetClient {
    client: reqwest::Client,
    options: RequestOptions,
    user_agent: String,
}

impl NetClient {
    pub fn new(config: &EngineConfig, options: RequestOptions) -> Result<Self> {
        let mut builder = reqwest::Client::builder()
            .user_agent(config.user_agent.clone())
            .redirect(reqwest::redirect::Policy::limited(config.max_redirects))
            .connect_timeout(Duration::from_secs(15))
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(usize::from(config.max_connections))
            .tcp_keepalive(Duration::from_secs(60))
            // Ranged transfers must operate on the identity representation.
            .gzip(false)
            .brotli(false)
            .danger_accept_invalid_certs(options.accept_invalid_certs);
        if let Some(proxy) = &options.proxy {
            builder = builder.proxy(
                reqwest::Proxy::all(proxy)
                    .map_err(|error| NetError::Config(format!("invalid proxy: {error}")))?,
            );
        }
        let client = builder
            .build()
            .map_err(|error| NetError::Config(error.to_string()))?;
        Ok(Self {
            client,
            options,
            user_agent: config.user_agent.clone(),
        })
    }

    #[must_use]
    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    #[must_use]
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    #[must_use]
    pub(crate) fn basic_auth(&self) -> Option<&(String, String)> {
        self.options.basic_auth.as_ref()
    }

    pub(crate) fn apply_common(
        &self,
        mut request: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder> {
        request = request.header(reqwest::header::ACCEPT_ENCODING, "identity");
        for (name, value) in &self.options.headers {
            let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|error| NetError::Header(format!("{name}: {error}")))?;
            let value = reqwest::header::HeaderValue::from_str(value)
                .map_err(|error| NetError::Header(format!("{name}: {error}")))?;
            request = request.header(name, value);
        }
        if let Some((user, pass)) = &self.options.basic_auth {
            request = request.basic_auth(user, Some(pass));
        }
        if let Some(token) = &self.options.bearer_token {
            request = request.bearer_auth(token);
        }
        if let Some(cookie) = &self.options.cookie {
            request = request.header(reqwest::header::COOKIE, cookie);
        }
        Ok(request)
    }

    /// Probe with HEAD, then confirm range support with a one-byte GET when
    /// the HEAD response is inconclusive or dishonest.
    pub async fn probe(&self, url: &Url, config: &EngineConfig) -> Result<FileProbe> {
        let deadline = Duration::from_secs(config.probe_timeout_secs.max(1));
        let head = tokio::time::timeout(
            deadline,
            self.apply_common(self.client.head(url.clone()))?.send(),
        )
        .await
        .map_err(|_| NetError::Protocol("probe timed out".into()))?;

        let head = match head {
            Ok(response) => Some(response),
            Err(error) if error.is_connect() || error.is_timeout() => {
                return Err(NetError::Request(error));
            }
            Err(error) => return Err(NetError::Request(error)),
        };
        if let Some(response) = &head {
            let status = response.status();
            if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::PROXY_AUTHENTICATION_REQUIRED
            {
                return Err(NetError::status(
                    status.as_u16(),
                    url.as_str(),
                    "authentication required",
                ));
            }
        }

        let head_usable = head.as_ref().is_some_and(|response| {
            response.status().is_success()
                && response
                    .headers()
                    .get(reqwest::header::CONTENT_LENGTH)
                    .is_some()
        });
        let advertised = head.as_ref().is_some_and(|response| {
            response
                .headers()
                .get(reqwest::header::ACCEPT_RANGES)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.to_ascii_lowercase().contains("bytes"))
        });
        if head_usable && advertised {
            let response = head.expect("checked above");
            return self.probe_from_response(url, response, true, None).await;
        }

        // GET 0-0 both confirms ranges and recovers a missing Content-Length.
        let get = tokio::time::timeout(
            deadline,
            self.apply_common(
                self.client
                    .get(url.clone())
                    .header(reqwest::header::RANGE, "bytes=0-0"),
            )?
            .send(),
        )
        .await
        .map_err(|_| NetError::Protocol("range probe timed out".into()))??;
        let status = get.status();
        if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            let total = get
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.rsplit('/').next())
                .and_then(|value| value.parse().ok());
            return self
                .probe_from_response_with_total(url, get, true, total)
                .await;
        }
        if !status.is_success() {
            return Err(NetError::status(
                status.as_u16(),
                url.as_str(),
                status.canonical_reason().unwrap_or("probe failed"),
            ));
        }
        let ranged = status == reqwest::StatusCode::PARTIAL_CONTENT;
        let total = if ranged {
            get.headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|value| value.to_str().ok())
                .and_then(parse_content_total)
        } else {
            None
        };
        self.probe_from_response(url, get, ranged, total).await
    }

    async fn probe_from_response(
        &self,
        requested: &Url,
        response: reqwest::Response,
        ranged: bool,
        override_total: Option<u64>,
    ) -> Result<FileProbe> {
        self.probe_from_response_with_total(requested, response, ranged, override_total)
            .await
    }

    async fn probe_from_response_with_total(
        &self,
        requested: &Url,
        response: reqwest::Response,
        ranged: bool,
        override_total: Option<u64>,
    ) -> Result<FileProbe> {
        let status = response.status();
        let final_url = response.url().clone();
        let headers = response.headers().clone();
        let encoding = headers
            .get(reqwest::header::CONTENT_ENCODING)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        if let Some(encoding) = encoding {
            if !encoding.eq_ignore_ascii_case("identity") {
                return Err(NetError::ContentEncoding(encoding));
            }
        }
        let content_range = headers
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let content_length = headers
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        let total = override_total
            .or_else(|| content_range.as_deref().and_then(parse_content_total))
            .or(content_length);
        let range_supported = ranged
            || headers
                .get(reqwest::header::ACCEPT_RANGES)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.to_ascii_lowercase().contains("bytes"));
        let etag = header_string(&headers, reqwest::header::ETAG);
        let last_modified = header_string(&headers, reqwest::header::LAST_MODIFIED);
        let content_type = header_string(&headers, reqwest::header::CONTENT_TYPE);
        let content_disposition = header_string(&headers, reqwest::header::CONTENT_DISPOSITION);
        let filename = content_disposition
            .as_deref()
            .and_then(rustget_storage::content_disposition_filename)
            .or_else(|| rustget_storage::filename_from_url(&final_url))
            .unwrap_or_else(|| "download".into());
        Ok(FileProbe {
            requested_url: requested.clone(),
            final_url,
            total_size: total,
            range_supported,
            validator: Validator {
                etag,
                last_modified,
            },
            content_type,
            content_disposition,
            filename,
            http_version: format!("{:?}", response.version()),
            status: status.as_u16(),
        })
    }
}

fn header_string(
    headers: &reqwest::header::HeaderMap,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn parse_content_total(value: &str) -> Option<u64> {
    parse_content_range(value)?.2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc7233_totals() {
        assert_eq!(parse_content_total("bytes 0-0/1234"), Some(1234));
        assert_eq!(parse_content_total("bytes 0-0/*"), None);
        assert_eq!(parse_content_total("garbage"), None);
    }
}
