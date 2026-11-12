//! Browser native-messaging host.
//!
//! Wire format is the Chrome/Firefox standard: a 32-bit native-endian length
//! followed by that many UTF-8 JSON bytes. The host only talks to the RustGet
//! loopback API with the profile's bearer token; it never executes URLs.
#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const MAX_MESSAGE: usize = 4 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request {
    Ping,
    Add {
        url: String,
        #[serde(default)]
        referrer: Option<String>,
        #[serde(default)]
        cookie: Option<String>,
        #[serde(default)]
        filename: Option<String>,
    },
    List,
    Status {
        id: String,
    },
    Pause {
        id: String,
    },
    Resume {
        id: String,
    },
}

#[derive(Debug, Serialize)]
struct Response {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

struct Host {
    base: String,
    token: String,
    client: reqwest::Client,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();
    let base = std::env::var("RUSTGET_API").unwrap_or_else(|_| "http://127.0.0.1:8765".into());
    let token = read_token()
        .context("cannot read the RustGet API token; start `rustget serve` once to create it")?;
    let host = Host {
        base,
        token,
        client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(45))
            .build()?,
    };
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();
    loop {
        let message = match read_message(&mut stdin) {
            Ok(Some(message)) => message,
            Ok(None) => break,
            Err(error) => {
                tracing::warn!(%error, "invalid native message");
                break;
            }
        };
        let response = match serde_json::from_slice::<Request>(&message) {
            Ok(request) => host.handle(request).await,
            Err(error) => Response {
                ok: false,
                error: Some(format!("invalid request: {error}")),
                data: None,
            },
        };
        let encoded = serde_json::to_vec(&response)?;
        write_message(&mut stdout, &encoded)?;
    }
    Ok(())
}

impl Host {
    async fn handle(&self, request: Request) -> Response {
        match self.handle_inner(request).await {
            Ok(data) => Response {
                ok: true,
                error: None,
                data: Some(data),
            },
            Err(error) => Response {
                ok: false,
                error: Some(error.to_string()),
                data: None,
            },
        }
    }

    async fn handle_inner(&self, request: Request) -> Result<Value> {
        match request {
            Request::Ping => Ok(json!({"version": env!("CARGO_PKG_VERSION"), "api": self.base})),
            Request::Add {
                url,
                referrer,
                cookie,
                filename,
            } => {
                let parsed = url::Url::parse(&url).context("invalid URL")?;
                if !matches!(parsed.scheme(), "http" | "https") {
                    bail!("only http(s) URLs are accepted");
                }
                let mut headers = Vec::new();
                if let Some(referrer) = referrer {
                    headers.push(("Referer".to_string(), referrer));
                }
                if let Some(cookie) = cookie {
                    headers.push(("Cookie".to_string(), cookie));
                }
                let body = json!({
                    "url": url,
                    "headers": headers,
                    "destination": null,
                    "start": true,
                    "category": "Browser",
                });
                let _ = filename;
                self.post("/api/v1/downloads", &body).await
            }
            Request::List => self.get("/api/v1/downloads").await,
            Request::Status { id } => {
                self.get(&format!("/api/v1/downloads/{}", safe_id(&id)?))
                    .await
            }
            Request::Pause { id } => {
                self.post_empty(&format!("/api/v1/downloads/{}/pause", safe_id(&id)?))
                    .await
            }
            Request::Resume { id } => {
                self.post_empty(&format!("/api/v1/downloads/{}/resume", safe_id(&id)?))
                    .await
            }
        }
    }

    async fn get(&self, path: &str) -> Result<Value> {
        let response = self
            .client
            .get(format!("{}{path}", self.base.trim_end_matches('/')))
            .bearer_auth(&self.token)
            .send()
            .await?;
        decode(response).await
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let response = self
            .client
            .post(format!("{}{path}", self.base.trim_end_matches('/')))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await?;
        decode(response).await
    }

    async fn post_empty(&self, path: &str) -> Result<Value> {
        let response = self
            .client
            .post(format!("{}{path}", self.base.trim_end_matches('/')))
            .bearer_auth(&self.token)
            .send()
            .await?;
        decode(response).await
    }
}

async fn decode(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        bail!("API {status}: {text}");
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

fn safe_id(id: &str) -> Result<String> {
    if id.len() == 36 && id.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-') {
        Ok(id.to_string())
    } else {
        bail!("invalid download id")
    }
}

fn read_token() -> Result<String> {
    let data_dir = if let Ok(dir) = std::env::var("RUSTGET_DATA_DIR") {
        PathBuf::from(dir)
    } else {
        dirs::data_dir()
            .context("no user data directory")?
            .join("RustGet")
    };
    let token = std::fs::read_to_string(data_dir.join("api-token"))?;
    let token = token.trim().to_string();
    if token.len() < 32 {
        bail!("API token is malformed");
    }
    Ok(token)
}

fn read_message(reader: &mut impl Read) -> Result<Option<Vec<u8>>> {
    let mut length = [0_u8; 4];
    match reader.read_exact(&mut length) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let length = u32::from_ne_bytes(length) as usize;
    if length == 0 || length > MAX_MESSAGE {
        bail!("native message length {length} is out of range");
    }
    let mut buffer = vec![0_u8; length];
    reader.read_exact(&mut buffer)?;
    Ok(Some(buffer))
}

fn write_message(writer: &mut impl Write, message: &[u8]) -> Result<()> {
    let length = u32::try_from(message.len()).context("message too large")?;
    writer.write_all(&length.to_ne_bytes())?;
    writer.write_all(message)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_round_trip() {
        let mut buffer = Vec::new();
        write_message(&mut buffer, b"{\"type\":\"ping\"}").unwrap();
        let mut cursor = std::io::Cursor::new(buffer);
        let message = read_message(&mut cursor).unwrap().unwrap();
        assert_eq!(message, b"{\"type\":\"ping\"}");
    }

    #[test]
    fn rejects_oversized_frame() {
        let mut cursor = std::io::Cursor::new(((MAX_MESSAGE + 1) as u32).to_ne_bytes().to_vec());
        assert!(read_message(&mut cursor).is_err());
    }
}
