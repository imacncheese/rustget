//! An instrumented, deliberately misbehaving HTTP origin used by RustGet's
//! integration tests and available as a standalone protocol lab.
//!
//! Run `cargo run -p rustget-testkit -- --size 1073741824` to serve a 1 GiB
//! deterministic file with honest range support and a request transcript at
//! `/__admin/requests`.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use bytes::Bytes;
use futures_util::stream;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tower_http::trace::TraceLayer;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestFile {
    pub name: String,
    pub size: u64,
    pub seed: u64,
    pub range_mode: RangeMode,
    pub redirect_hops: u8,
    pub flaky_failures: u32,
    pub fail_after: u64,
    pub throttle_bps: Option<u64>,
    pub basic_auth: Option<(String, String)>,
    pub content_disposition: Option<String>,
    /// Increment ETag on every request, simulating a file changed in place.
    pub changing_etag: bool,
    /// Omit Content-Length and send a chunked body.
    pub omit_length: bool,
    pub head_delay_ms: u64,
    /// Delay only the range that begins at byte zero. Useful for forcing an
    /// imbalance that exercises work stealing.
    pub slow_start_zero_ms: u64,
    pub etag: Option<String>,
}

impl TestFile {
    #[must_use]
    pub fn new(name: impl Into<String>, size: u64) -> Self {
        Self {
            name: name.into(),
            size,
            seed: 0x5EED_1234_ABCD_9876,
            range_mode: RangeMode::Honest,
            redirect_hops: 0,
            flaky_failures: 0,
            fail_after: 0,
            throttle_bps: None,
            basic_auth: None,
            content_disposition: None,
            changing_etag: false,
            omit_length: false,
            head_delay_ms: 0,
            slow_start_zero_ms: 0,
            etag: None,
        }
    }

    #[must_use]
    pub fn slow_first_range(mut self, delay_ms: u64) -> Self {
        self.slow_start_zero_ms = delay_ms;
        self
    }

    #[must_use]
    pub fn no_range(mut self) -> Self {
        self.range_mode = RangeMode::Ignore;
        self
    }

    #[must_use]
    pub fn lie_about_range(mut self) -> Self {
        self.range_mode = RangeMode::Lie;
        self
    }

    #[must_use]
    pub fn flaky(mut self, failures: u32, fail_after: u64) -> Self {
        self.flaky_failures = failures;
        self.fail_after = fail_after;
        self
    }

    #[must_use]
    pub fn throttled(mut self, bytes_per_sec: u64) -> Self {
        self.throttle_bps = Some(bytes_per_sec);
        self
    }

    #[must_use]
    pub fn redirects(mut self, hops: u8) -> Self {
        self.redirect_hops = hops;
        self
    }

    #[must_use]
    pub fn with_basic_auth(mut self, user: &str, pass: &str) -> Self {
        self.basic_auth = Some((user.into(), pass.into()));
        self
    }

    #[must_use]
    pub fn with_disposition(mut self, value: &str) -> Self {
        self.content_disposition = Some(value.into());
        self
    }

    #[must_use]
    pub fn changing_etag(mut self) -> Self {
        self.changing_etag = true;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RangeMode {
    /// Correct 206 responses with Content-Range.
    Honest,
    /// Ignore Range and return the whole file with 200.
    Ignore,
    /// Advertise `Accept-Ranges: bytes` but ignore Range.
    Lie,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestLog {
    pub method: String,
    pub path: String,
    pub range: Option<String>,
    pub if_range: Option<String>,
    pub user_agent: Option<String>,
    pub status: u16,
    pub range_start: Option<u64>,
    pub range_end: Option<u64>,
    pub peer: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TestkitConfig {
    pub files: Vec<TestFile>,
}

impl TestkitConfig {
    #[must_use]
    pub fn single(size: u64) -> Self {
        Self {
            files: vec![TestFile::new("file.bin", size)],
        }
    }

    #[must_use]
    pub fn with_file(mut self, file: TestFile) -> Self {
        self.files.push(file);
        self
    }
}

struct RuntimeFile {
    spec: TestFile,
    remaining_failures: AtomicU32,
    etag_generation: AtomicU64,
}

struct AppState {
    files: HashMap<String, Arc<RuntimeFile>>,
    log: std::sync::Mutex<Vec<RequestLog>>,
    requests: AtomicU64,
}

#[derive(Clone)]
struct Shared(Arc<AppState>);

pub struct TestServer {
    pub addr: SocketAddr,
    pub base_url: String,
    state: Arc<AppState>,
    shutdown: Option<oneshot::Sender<()>>,
}

impl TestServer {
    pub async fn start(config: TestkitConfig) -> std::io::Result<Self> {
        Self::start_at("127.0.0.1:0".parse().expect("valid loopback"), config).await
    }

    pub async fn start_at(listen: SocketAddr, config: TestkitConfig) -> std::io::Result<Self> {
        let files = config
            .files
            .into_iter()
            .map(|spec| {
                let runtime = RuntimeFile {
                    remaining_failures: AtomicU32::new(spec.flaky_failures),
                    etag_generation: AtomicU64::new(0),
                    spec,
                };
                (runtime.spec.name.clone(), Arc::new(runtime))
            })
            .collect();
        let state = Arc::new(AppState {
            files,
            log: std::sync::Mutex::new(Vec::new()),
            requests: AtomicU64::new(0),
        });
        let app = Router::new()
            .route("/files/{name}", get(serve_file).head(serve_file))
            .route("/redirect/{name}/{hop}", get(serve_redirect))
            .route("/__admin/requests", get(admin_requests))
            .route("/__admin/stats", get(admin_stats))
            .route("/__admin/reset", post(admin_reset))
            .layer(TraceLayer::new_for_http())
            .with_state(Shared(state.clone()));
        let listener = TcpListener::bind(listen).await?;
        let addr = listener.local_addr()?;
        let (tx, rx) = oneshot::channel();
        let server = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = rx.await;
        });
        tokio::spawn(async move {
            if let Err(error) = server.await {
                tracing::error!(%error, "testkit server stopped");
            }
        });
        Ok(Self {
            addr,
            base_url: format!("http://{addr}"),
            state,
            shutdown: Some(tx),
        })
    }

    #[must_use]
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    #[must_use]
    pub fn file_url(&self, name: &str) -> String {
        self.url(&format!("/files/{name}"))
    }

    pub fn requests(&self) -> Vec<RequestLog> {
        self.state.log.lock().expect("testkit lock").clone()
    }

    pub fn request_count(&self) -> u64 {
        self.state.requests.load(Ordering::Relaxed)
    }

    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

async fn admin_requests(State(state): State<Shared>) -> impl IntoResponse {
    axum::Json(state.0.log.lock().expect("testkit lock").clone())
}

async fn admin_stats(State(state): State<Shared>) -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "requests": state.0.requests.load(Ordering::Relaxed),
        "files": state.0.files.len(),
    }))
}

async fn admin_reset(State(state): State<Shared>) -> impl IntoResponse {
    state.0.log.lock().expect("testkit lock").clear();
    state.0.requests.store(0, Ordering::Relaxed);
    StatusCode::NO_CONTENT
}

async fn serve_redirect(
    State(state): State<Shared>,
    Path((name, hop)): Path<(String, u8)>,
) -> Response {
    if !state.0.files.contains_key(&name) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let next = if hop == 0 {
        format!("/files/{name}")
    } else {
        format!("/redirect/{name}/{}", hop - 1)
    };
    let mut response = StatusCode::TEMPORARY_REDIRECT.into_response();
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(&next).unwrap_or_else(|_| HeaderValue::from_static("/")),
    );
    response
}

async fn serve_file(
    State(state): State<Shared>,
    Path(name): Path<String>,
    method: Method,
    headers: HeaderMap,
    connect: axum::extract::ConnectInfo<SocketAddr>,
) -> Response {
    let Some(file) = state.0.files.get(&name).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if let Some((user, pass)) = &file.spec.basic_auth {
        let expected = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"))
        );
        let actual = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok());
        if actual != Some(expected.as_str()) {
            let mut response = StatusCode::UNAUTHORIZED.into_response();
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Basic realm=\"rustget-testkit\""),
            );
            return response;
        }
    }
    if file.spec.head_delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(file.spec.head_delay_ms)).await;
    }
    let range_header = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let if_range = headers
        .get(header::IF_RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let request_user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let parsed = range_header
        .as_deref()
        .and_then(|value| parse_range(value, file.spec.size));
    let generation = file.etag_generation.fetch_add(1, Ordering::SeqCst);
    let etag = if file.spec.changing_etag {
        format!("\"gen-{generation}\"")
    } else {
        file.spec
            .etag
            .clone()
            .unwrap_or_else(|| format!("\"{:x}-{:x}\"", file.spec.seed, file.spec.size))
    };
    const LAST_MODIFIED: &str = "Wed, 21 Oct 2015 07:28:00 GMT";
    let if_range_ok = if_range
        .as_ref()
        .is_none_or(|candidate| candidate == &etag || candidate == LAST_MODIFIED);
    let use_range = parsed.is_some()
        && file.spec.range_mode == RangeMode::Honest
        && (if_range.is_none() || if_range_ok);
    let (status, start, length, declared_end) = if use_range {
        let (start, end) = parsed.expect("checked above");
        let end = end.min(file.spec.size.saturating_sub(1));
        if start > end {
            let mut response = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
            response.headers_mut().insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes */{}", file.spec.size))
                    .unwrap_or_else(|_| HeaderValue::from_static("bytes */*")),
            );
            return response;
        }
        (
            StatusCode::PARTIAL_CONTENT,
            start,
            end - start + 1,
            Some(end),
        )
    } else {
        (StatusCode::OK, 0, file.spec.size, None)
    };
    if file.spec.slow_start_zero_ms > 0
        && parsed.is_some_and(|(start, _)| start == 0)
        && method != Method::HEAD
    {
        tokio::time::sleep(Duration::from_millis(file.spec.slow_start_zero_ms)).await;
    }
    let mut current = file.remaining_failures.load(Ordering::SeqCst);
    let flaky = loop {
        let Some(next) = current.checked_sub(1) else {
            break Err(());
        };
        match file.remaining_failures.compare_exchange(
            current,
            next,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => break Ok(()),
            Err(observed) => current = observed,
        }
    };
    let fail = flaky.is_ok();
    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        make_body(
            file.spec.seed,
            start,
            length,
            file.spec.throttle_bps,
            if fail {
                Some(file.spec.fail_after.min(length.saturating_sub(1)))
            } else {
                None
            },
        )
    };
    let mut response = Response::new(body);
    *response.status_mut() = status;
    {
        let response_headers = response.headers_mut();
        if file.spec.range_mode != RangeMode::Ignore {
            response_headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        }
        response_headers.insert(header::ETAG, header_value_or_star(&etag));
        response_headers.insert(
            header::LAST_MODIFIED,
            HeaderValue::from_static(LAST_MODIFIED),
        );
        response_headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
        if let Some(disposition) = &file.spec.content_disposition {
            if let Ok(value) = HeaderValue::from_str(disposition) {
                response_headers.insert(header::CONTENT_DISPOSITION, value);
            }
        }
        if !file.spec.omit_length {
            response_headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
        }
        if let Some(end) = declared_end {
            if let Ok(value) =
                HeaderValue::from_str(&format!("bytes {start}-{end}/{}", file.spec.size))
            {
                response_headers.insert(header::CONTENT_RANGE, value);
            }
        }
    }
    state.0.requests.fetch_add(1, Ordering::Relaxed);
    state.0.log.lock().expect("testkit lock").push(RequestLog {
        method: method.to_string(),
        path: format!("/files/{name}"),
        range: range_header,
        if_range,
        user_agent: request_user_agent,
        status: status.as_u16(),
        range_start: use_range.then_some(start),
        range_end: declared_end,
        peer: connect.0.to_string(),
    });
    response
}

fn header_value_or_star(value: &str) -> HeaderValue {
    HeaderValue::from_str(value).unwrap_or_else(|_| HeaderValue::from_static("\"invalid\""))
}

fn make_body(
    seed: u64,
    start: u64,
    length: u64,
    throttle_bps: Option<u64>,
    fail_after: Option<u64>,
) -> Body {
    let chunk_size = 64 * 1024_u64;
    let stream = stream::unfold(
        (seed, start, length, throttle_bps, fail_after, 0_u64),
        move |(seed, start, length, throttle_bps, fail_after, sent)| async move {
            if sent >= length {
                return None;
            }
            if let Some(limit) = fail_after {
                if sent >= limit {
                    return Some((
                        Err(std::io::Error::new(
                            std::io::ErrorKind::ConnectionReset,
                            "testkit injected mid-stream reset",
                        )),
                        (seed, start, length, throttle_bps, fail_after, sent),
                    ));
                }
            }
            let allowed =
                fail_after.map_or(length - sent, |limit| (limit - sent).min(length - sent));
            let n = chunk_size.min(length - sent).min(allowed);
            if n == 0 {
                return Some((
                    Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionReset,
                        "testkit injected mid-stream reset",
                    )),
                    (seed, start, length, throttle_bps, fail_after, sent),
                ));
            }
            if let Some(bps) = throttle_bps {
                tokio::time::sleep(Duration::from_secs_f64(n as f64 / bps as f64)).await;
            }
            let data = generate(seed, start + sent, n);
            Some((
                Ok::<_, std::io::Error>(Bytes::from(data)),
                (seed, start, length, throttle_bps, fail_after, sent + n),
            ))
        },
    );
    Body::from_stream(stream)
}

/// Deterministic content so clients can verify byte-exact output without
/// allocating the whole file on either side.
const GEN_BLOCK: u64 = 64 * 1024;

/// Random-access deterministic content. Bytes depend only on `seed` and the
/// absolute offset, so a ranged response matches the corresponding slice of
/// the complete file without materializing it.
#[must_use]
pub fn generate(seed: u64, offset: u64, length: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(usize::try_from(length).unwrap_or(0));
    let mut cursor = offset;
    let end = offset.saturating_add(length);
    while cursor < end {
        let block_index = cursor / GEN_BLOCK;
        let in_block = cursor % GEN_BLOCK;
        let take = (GEN_BLOCK - in_block).min(end - cursor);
        let block = make_block(seed, block_index);
        out.extend_from_slice(
            &block[usize::try_from(in_block).unwrap_or(0)
                ..usize::try_from(in_block + take).unwrap_or(0)],
        );
        cursor += take;
    }
    out
}

fn make_block(seed: u64, block_index: u64) -> [u8; GEN_BLOCK as usize] {
    let mut state = seed
        ^ block_index
            .wrapping_add(1)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut block = [0_u8; GEN_BLOCK as usize];
    for byte in &mut block {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = (state & 0xFF) as u8;
    }
    block
}

#[must_use]
pub fn expected_content(seed: u64, size: u64) -> Vec<u8> {
    generate(seed, 0, size)
}

fn parse_range(value: &str, total: u64) -> Option<(u64, u64)> {
    let value = value.strip_prefix("bytes=")?;
    if value.contains(',') || total == 0 {
        return None;
    }
    let (start, end) = value.split_once('-')?;
    if start.is_empty() {
        let suffix: u64 = end.parse().ok()?;
        let start = total.saturating_sub(suffix);
        return Some((start, total - 1));
    }
    let start: u64 = start.parse().ok()?;
    let end = if end.is_empty() {
        total.saturating_sub(1)
    } else {
        end.parse().ok()?
    };
    (start <= end).then_some((start, end.min(total.saturating_sub(1))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_parser_handles_open_and_suffix_ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=900-", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=0-1999", 1000), Some((0, 999)));
        assert_eq!(parse_range("bytes=1000-", 1000), None);
    }

    #[tokio::test]
    async fn serves_correct_partial_content_and_logs() {
        let server = TestServer::start(TestkitConfig::single(4096))
            .await
            .unwrap();
        let client = reqwest::Client::new();
        let response = client
            .get(server.file_url("file.bin"))
            .header("Range", "bytes=100-199")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 206);
        let bytes = response.bytes().await.unwrap();
        assert_eq!(
            &bytes[..],
            &expected_content(0x5EED_1234_ABCD_9876, 4096)[100..200]
        );
        let log = server.requests();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].range.as_deref(), Some("bytes=100-199"));
    }

    #[tokio::test]
    async fn no_range_mode_returns_full_200() {
        let server = TestServer::start(
            TestkitConfig::default().with_file(TestFile::new("plain.bin", 512).no_range()),
        )
        .await
        .unwrap();
        let response = reqwest::Client::new()
            .get(server.file_url("plain.bin"))
            .header("Range", "bytes=0-99")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.bytes().await.unwrap().len(), 512);
    }

    #[tokio::test]
    async fn flaky_mode_resets_mid_stream_then_recovers() {
        let server = TestServer::start(
            TestkitConfig::default().with_file(
                TestFile::new("flaky.bin", 256 * 1024)
                    .flaky(1, 32 * 1024)
                    .throttled(100_000_000),
            ),
        )
        .await
        .unwrap();
        let client = reqwest::Client::new();
        let first = client
            .get(server.file_url("flaky.bin"))
            .send()
            .await
            .unwrap();
        assert_eq!(first.status(), 200);
        let error = first.bytes().await.unwrap_err();
        assert!(error.is_body() || error.is_decode());
        let second = client
            .get(server.file_url("flaky.bin"))
            .send()
            .await
            .unwrap();
        assert_eq!(second.bytes().await.unwrap().len(), 256 * 1024);
    }

    #[tokio::test]
    async fn redirect_chain_lands_on_file() {
        let server = TestServer::start(
            TestkitConfig::default().with_file(TestFile::new("r.bin", 32).redirects(2)),
        )
        .await
        .unwrap();
        let response = reqwest::get(server.url("/redirect/r.bin/1")).await.unwrap();
        // The direct endpoint is /files; the redirect helper points at /files
        // for hop 0. A two-hop spec is exercised through the engine later.
        assert!(response.status().is_success());
    }
}
