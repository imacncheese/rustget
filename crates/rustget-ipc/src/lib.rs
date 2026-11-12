//! Local-only HTTP API and web UI host.
//!
//! Security model: bind loopback, require a per-profile bearer token, reject
//! cross-origin mutations, never enable CORS, and cap request bodies.
#![forbid(unsafe_code)]

use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, extract::Request};
use futures_util::{SinkExt, StreamExt};
use rustget_queue::{DownloadManager, DownloadStatus, NewDownload, PowerAction, Schedule};
use serde::Deserialize;
use serde_json::json;
use tokio::net::TcpListener;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden origin")]
    ForbiddenOrigin,
    #[error("not found")]
    NotFound,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("server error: {0}")]
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::ForbiddenOrigin => (StatusCode::FORBIDDEN, "forbidden_origin"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        };
        (
            status,
            Json(json!({"error": self.to_string(), "code": code})),
        )
            .into_response()
    }
}

#[derive(Clone)]
pub struct ApiState {
    manager: Arc<DownloadManager>,
    token: Arc<String>,
    allowed_origins: Arc<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub listen: SocketAddr,
    pub token: String,
    pub allowed_origins: Vec<String>,
}

impl ApiConfig {
    pub fn loopback(token: String) -> Self {
        Self {
            listen: SocketAddr::new(IpAddr::from([127, 0, 0, 1]), 8765),
            token,
            allowed_origins: Vec::new(),
        }
    }
}

/// Read or create the bearer token for this profile.
pub fn load_or_create_token(data_dir: &Path) -> std::io::Result<String> {
    std::fs::create_dir_all(data_dir)?;
    let path = data_dir.join("api-token");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let trimmed = existing.trim();
        if trimmed.len() >= 32 {
            return Ok(trimmed.to_string());
        }
    }
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    std::fs::write(&path, &token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(token)
}

pub fn router(manager: Arc<DownloadManager>, config: &ApiConfig) -> Router {
    let state = ApiState {
        manager,
        token: Arc::new(config.token.clone()),
        allowed_origins: Arc::new(config.allowed_origins.clone()),
    };
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/downloads", get(list_downloads).post(add_download))
        .route(
            "/api/v1/downloads/{id}",
            get(get_download)
                .patch(patch_download)
                .delete(remove_download),
        )
        .route("/api/v1/downloads/pause-all", post(pause_all))
        .route("/api/v1/downloads/resume-all", post(resume_all))
        .route("/api/v1/downloads/{id}/pause", post(pause_download))
        .route("/api/v1/downloads/{id}/resume", post(resume_download))
        .route("/api/v1/downloads/{id}/retry", post(retry_download))
        .route("/api/v1/downloads/{id}/open", post(open_download))
        .route("/api/v1/export", get(export_downloads))
        .route("/api/v1/import", post(import_downloads))
        .route("/api/v1/stats", get(stats))
        .route("/api/v1/categories", get(list_categories))
        .route("/api/v1/schedules", get(list_schedules).post(add_schedule))
        .route(
            "/api/v1/schedules/{id}",
            axum::routing::delete(remove_schedule),
        )
        .route("/api/v1/ws", get(ws_handler))
        .route("/api/v1/openapi.json", get(openapi))
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style_css))
        .route("/favicon.svg", get(favicon))
        .fallback(get(index))
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

pub async fn serve(manager: Arc<DownloadManager>, config: ApiConfig) -> std::io::Result<()> {
    let app = router(manager, &config);
    let listener = TcpListener::bind(config.listen).await?;
    let addr = listener.local_addr()?;
    tracing::info!(%addr, "RustGet API listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn guard(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let path = request.uri().path();
    if !path.starts_with("/api/v1/") || path == "/api/v1/health" {
        return Ok(next.run(request).await);
    }
    if path == "/api/v1/ws" {
        return check_ws_token(&state, request, next).await;
    }
    check_bearer(&state, request.headers())?;
    if request.method() != axum::http::Method::GET {
        check_origin(&state, request.headers())?;
    }
    if !peer.ip().is_loopback() {
        return Err(ApiError::ForbiddenOrigin);
    }
    Ok(next.run(request).await)
}

fn check_bearer(state: &ApiState, headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Err(ApiError::Unauthorized);
    };
    let Ok(value) = value.to_str() else {
        return Err(ApiError::Unauthorized);
    };
    let Some(token) = value.strip_prefix("Bearer ") else {
        return Err(ApiError::Unauthorized);
    };
    if constant_time_eq(token.trim().as_bytes(), state.token.as_bytes()) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized)
    }
}

fn check_origin(state: &ApiState, headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return Ok(());
    };
    if state
        .allowed_origins
        .iter()
        .any(|allowed| allowed == origin)
    {
        return Ok(());
    }
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return Err(ApiError::ForbiddenOrigin);
    };
    let expected = format!("http://{host}");
    let expected_https = format!("https://{host}");
    if origin == expected || origin == expected_https {
        Ok(())
    } else {
        Err(ApiError::ForbiddenOrigin)
    }
}

async fn check_ws_token(
    state: &ApiState,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let query = request.uri().query().unwrap_or_default();
    let mut token = None;
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("token=") {
            token = Some(percent_decode(value));
        }
    }
    let authorized =
        token.is_some_and(|token| constant_time_eq(token.as_bytes(), state.token.as_bytes()));
    if !authorized {
        return Err(ApiError::Unauthorized);
    }
    Ok(next.run(request).await)
}

fn percent_decode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(char::from(byte));
                index += 3;
                continue;
            }
        }
        out.push(char::from(bytes[index]));
        index += 1;
    }
    out
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0_u8;
    for (a, b) in left.iter().zip(right) {
        diff |= a ^ b;
    }
    diff == 0
}

async fn health() -> impl IntoResponse {
    Json(json!({"status": "ok", "version": env!("CARGO_PKG_VERSION")}))
}

async fn list_downloads(
    State(state): State<ApiState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let status = query.status.as_deref().map(DownloadStatus::parse);
    let items = state
        .manager
        .list(status)
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    Ok(Json(json!({"downloads": items, "count": items.len()})))
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    status: Option<String>,
}

async fn add_download(
    State(state): State<ApiState>,
    Json(input): Json<NewDownload>,
) -> Result<(StatusCode, Json<rustget_queue::DownloadRecord>), ApiError> {
    let record = state
        .manager
        .add(input)
        .await
        .map_err(|error| map_manager_error(&error))?;
    Ok((StatusCode::CREATED, Json(record)))
}

async fn get_download(
    State(state): State<ApiState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let id = parse_id(&id)?;
    let record = state
        .manager
        .get(id)
        .map_err(|error| map_manager_error(&error))?;
    let progress = state.manager.live_progress(id);
    Ok(Json(json!({"download": record, "progress": progress})))
}

async fn pause_all(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let count = state.manager.pause_all();
    Json(json!({"paused": count}))
}

async fn resume_all(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let count = state.manager.resume_all();
    Json(json!({"resumed": count}))
}

#[derive(Debug, Deserialize)]
struct PatchDownload {
    priority: Option<i32>,
}

async fn patch_download(
    State(state): State<ApiState>,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<PatchDownload>,
) -> Result<Json<rustget_queue::DownloadRecord>, ApiError> {
    let id = parse_id(&id)?;
    if let Some(priority) = input.priority {
        return state
            .manager
            .set_priority(id, priority)
            .map(Json)
            .map_err(|error| map_manager_error(&error));
    }
    Err(ApiError::BadRequest("no supported fields supplied".into()))
}

async fn pause_download(
    State(state): State<ApiState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<rustget_queue::DownloadRecord>, ApiError> {
    let id = parse_id(&id)?;
    state
        .manager
        .pause(id)
        .map(Json)
        .map_err(|error| map_manager_error(&error))
}

async fn resume_download(
    State(state): State<ApiState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<rustget_queue::DownloadRecord>, ApiError> {
    let id = parse_id(&id)?;
    state
        .manager
        .resume(id)
        .map(Json)
        .map_err(|error| map_manager_error(&error))
}

async fn retry_download(
    State(state): State<ApiState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<rustget_queue::DownloadRecord>, ApiError> {
    let id = parse_id(&id)?;
    state
        .manager
        .retry(id)
        .map(Json)
        .map_err(|error| map_manager_error(&error))
}

#[derive(Debug, Deserialize)]
struct RemoveQuery {
    #[serde(default)]
    delete_files: bool,
}

async fn remove_download(
    State(state): State<ApiState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<RemoveQuery>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&id)?;
    state
        .manager
        .remove(id, query.delete_files)
        .map_err(|error| map_manager_error(&error))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn open_download(
    State(state): State<ApiState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&id)?;
    let record = state
        .manager
        .get(id)
        .map_err(|error| map_manager_error(&error))?;
    let target = if record.destination.exists() {
        record.destination
    } else {
        record
            .destination
            .parent()
            .unwrap_or(&record.destination)
            .to_path_buf()
    };
    open::that_detached(&target).map_err(|error| ApiError::Internal(error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_categories(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let categories = state
        .manager
        .categories()
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    Ok(Json(json!({"categories": categories})))
}

async fn export_downloads(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let items = state
        .manager
        .list(None)
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    let downloads: Vec<NewDownload> = items.iter().map(NewDownload::from_record).collect();
    Ok(Json(json!({
        "version": 1,
        "downloads": downloads,
    })))
}

#[derive(Debug, Deserialize)]
struct ImportBundle {
    #[serde(default)]
    downloads: Vec<NewDownload>,
}

async fn import_downloads(
    State(state): State<ApiState>,
    Json(bundle): Json<ImportBundle>,
) -> Json<serde_json::Value> {
    let mut imported = 0_usize;
    let mut failed = Vec::new();
    for mut entry in bundle.downloads {
        entry.start = false;
        match state.manager.add(entry).await {
            Ok(_) => imported += 1,
            Err(error) => failed.push(error.to_string()),
        }
    }
    Json(json!({"imported": imported, "failed": failed}))
}

async fn stats(State(state): State<ApiState>) -> Result<Json<serde_json::Value>, ApiError> {
    let items = state
        .manager
        .list(None)
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    let active = items.iter().filter(|item| item.status.is_active()).count();
    let completed = items
        .iter()
        .filter(|item| item.status == DownloadStatus::Completed)
        .count();
    let failed = items
        .iter()
        .filter(|item| item.status == DownloadStatus::Failed)
        .count();
    let total_speed: f64 = items.iter().map(|item| item.speed_bps).sum();
    Ok(Json(json!({
        "total": items.len(),
        "active": active,
        "completed": completed,
        "failed": failed,
        "total_speed_bps": total_speed,
    })))
}

#[derive(Debug, Deserialize)]
struct NewSchedule {
    name: String,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    start: Option<String>,
    #[serde(default)]
    stop: Option<String>,
    /// Comma-separated weekday names or a bitmask integer.
    #[serde(default = "default_days")]
    days: serde_json::Value,
    #[serde(default = "default_action")]
    action: String,
}

fn default_true() -> bool {
    true
}
fn default_days() -> serde_json::Value {
    serde_json::Value::String("mon,tue,wed,thu,fri,sat,sun".into())
}
fn default_action() -> String {
    "none".into()
}

fn parse_days(value: &serde_json::Value) -> Result<u8, ApiError> {
    if let Some(number) = value.as_u64() {
        return u8::try_from(number).map_err(|_| ApiError::BadRequest("invalid days mask".into()));
    }
    let Some(text) = value.as_str() else {
        return Err(ApiError::BadRequest("days must be a mask or list".into()));
    };
    let mut mask = 0_u8;
    for part in text.split(',') {
        let bit = match part.trim().to_ascii_lowercase().as_str() {
            "mon" | "monday" => 0,
            "tue" | "tuesday" => 1,
            "wed" | "wednesday" => 2,
            "thu" | "thursday" => 3,
            "fri" | "friday" => 4,
            "sat" | "saturday" => 5,
            "sun" | "sunday" => 6,
            "" => continue,
            other => {
                return Err(ApiError::BadRequest(format!("unknown weekday '{other}'")));
            }
        };
        mask |= 1 << bit;
    }
    if mask == 0 {
        return Err(ApiError::BadRequest(
            "at least one weekday is required".into(),
        ));
    }
    Ok(mask)
}

async fn list_schedules(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let schedules = state
        .manager
        .schedules()
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    Ok(Json(json!({"schedules": schedules})))
}

async fn add_schedule(
    State(state): State<ApiState>,
    Json(input): Json<NewSchedule>,
) -> Result<(StatusCode, Json<Schedule>), ApiError> {
    let start = input
        .start
        .as_deref()
        .map(|value| {
            Schedule::parse_time(value)
                .ok_or_else(|| ApiError::BadRequest("start must be HH:MM".into()))
        })
        .transpose()?;
    let stop = input
        .stop
        .as_deref()
        .map(|value| {
            Schedule::parse_time(value)
                .ok_or_else(|| ApiError::BadRequest("stop must be HH:MM".into()))
        })
        .transpose()?;
    let schedule = Schedule {
        id: Uuid::new_v4(),
        name: input.name,
        enabled: input.enabled,
        days: parse_days(&input.days)?,
        start_min: start,
        stop_min: stop,
        action: PowerAction::parse(&input.action),
    };
    state
        .manager
        .add_schedule(schedule.clone())
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    Ok((StatusCode::CREATED, Json(schedule)))
}

async fn remove_schedule(
    State(state): State<ApiState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&id)?;
    state
        .manager
        .remove_schedule(id)
        .map_err(|error| ApiError::Internal(error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn ws_handler(State(state): State<ApiState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| websocket(socket, state))
}

async fn websocket(socket: WebSocket, state: ApiState) {
    let (mut sender, mut receiver) = socket.split();
    let mut events = state.manager.subscribe();
    let initial = state.manager.list(None).unwrap_or_default();
    let hello = json!({"type": "hello", "downloads": initial});
    if sender
        .send(Message::Text(hello.to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    loop {
        tokio::select! {
            event = events.recv() => {
                let Ok(event) = event else { break };
                let payload = serde_json::to_string(&event).unwrap_or_else(|_| "{}".into());
                if sender.send(Message::Text(payload.into())).await.is_err() {
                    break;
                }
            }
            message = receiver.next() => {
                match message {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(payload))) => {
                        if sender.send(Message::Pong(payload)).await.is_err() { break; }
                    }
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

fn parse_id(value: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(value).map_err(|_| ApiError::BadRequest("invalid download id".into()))
}

fn map_manager_error(error: &rustget_queue::manager::ManagerError) -> ApiError {
    use rustget_queue::manager::ManagerError;
    match error {
        ManagerError::NotFound(_) => ApiError::NotFound,
        ManagerError::Url(_)
        | ManagerError::InvalidState(_)
        | ManagerError::Net(rustget_net::NetError::InvalidUrl(_)) => {
            ApiError::BadRequest(error.to_string())
        }
        _ => ApiError::Internal(error.to_string()),
    }
}

async fn index() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        rustget_app::INDEX_HTML,
    )
}
async fn app_js() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        rustget_app::APP_JS,
    )
}
async fn style_css() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        rustget_app::STYLE_CSS,
    )
}
async fn favicon() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        rustget_app::FAVICON_SVG,
    )
}

async fn openapi() -> impl IntoResponse {
    Json(json!({
        "openapi": "3.1.0",
        "info": {"title": "RustGet local API", "version": env!("CARGO_PKG_VERSION")},
        "servers": [{"url": "/"}],
        "components": {"securitySchemes": {"bearer": {"type": "http", "scheme": "bearer"}}},
        "security": [{"bearer": []}],
        "paths": {
            "/api/v1/health": {"get": {"security": [], "summary": "Liveness"}},
            "/api/v1/downloads": {
                "get": {"summary": "List downloads"},
                "post": {"summary": "Add and optionally start a download"}
            },
            "/api/v1/downloads/{id}": {
                "get": {"summary": "Get one download"},
                "delete": {"summary": "Remove a download"}
            },
            "/api/v1/downloads/{id}/pause": {"post": {"summary": "Pause"}},
            "/api/v1/downloads/{id}/resume": {"post": {"summary": "Resume"}},
            "/api/v1/downloads/{id}/retry": {"post": {"summary": "Retry failed"}},
            "/api/v1/downloads/{id}/open": {"post": {"summary": "Open containing folder"}},
            "/api/v1/stats": {"get": {"summary": "Aggregate statistics"}},
            "/api/v1/ws": {"get": {"summary": "Event stream (token query parameter)"}}
        }
    }))
}

/// Extract the token from a URL fragment for CLI printing.
#[must_use]
pub fn browser_url(addr: SocketAddr, token: &str) -> String {
    format!("http://{addr}/#token={token}")
}

#[must_use]
pub fn is_loopback(addr: SocketAddr) -> bool {
    addr.ip().is_loopback()
}
