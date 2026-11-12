use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rustget_core::EngineConfig;
use rustget_net::{
    ChecksumSpec, DownloadProgress, DownloadRequest, DownloadState, Downloader, HostLimiter,
    NetError, RequestOptions,
};
use rustget_storage::{DownloadPaths, sanitize_filename, unique_destination};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

use crate::db::Database;
use crate::model::{DownloadRecord, DownloadStatus};

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("network error: {0}")]
    Net(#[from] NetError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("download {0} not found")]
    NotFound(Uuid),
    #[error("invalid state transition: {0}")]
    InvalidState(String),
}

pub type Result<T, E = ManagerError> = std::result::Result<T, E>;

#[derive(Debug, Clone)]
pub struct ManagerConfig {
    pub data_dir: PathBuf,
    pub download_dir: PathBuf,
    pub engine: EngineConfig,
    pub max_concurrent: usize,
}

impl ManagerConfig {
    #[must_use]
    pub fn new(data_dir: PathBuf, download_dir: PathBuf) -> Self {
        Self {
            data_dir,
            download_dir,
            engine: EngineConfig::default(),
            max_concurrent: 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDownload {
    pub url: String,
    #[serde(default)]
    pub mirrors: Vec<String>,
    pub destination: Option<PathBuf>,
    pub directory: Option<PathBuf>,
    pub category: Option<String>,
    pub connections: Option<u8>,
    pub speed_limit: Option<u64>,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    pub checksum_algorithm: Option<String>,
    pub checksum_value: Option<String>,
    pub priority: Option<i32>,
    #[serde(default = "default_true")]
    pub start: bool,
}

fn default_true() -> bool {
    true
}

impl NewDownload {
    /// Convert an existing row into a portable, non-starting queue entry.
    #[must_use]
    pub fn from_record(record: &DownloadRecord) -> Self {
        Self {
            url: record.url.clone(),
            mirrors: record.mirrors.clone(),
            destination: Some(record.destination.clone()),
            directory: record
                .destination
                .parent()
                .map(std::path::Path::to_path_buf),
            category: Some(record.category.clone()),
            connections: Some(record.connections),
            speed_limit: record.speed_limit,
            headers: record.headers.clone(),
            checksum_algorithm: record.checksum_algorithm.clone(),
            checksum_value: record.checksum_value.clone(),
            priority: Some(record.priority),
            start: false,
        }
    }
}

impl Default for NewDownload {
    fn default() -> Self {
        Self {
            url: String::new(),
            mirrors: Vec::new(),
            destination: None,
            directory: None,
            category: None,
            connections: None,
            speed_limit: None,
            headers: Vec::new(),
            checksum_algorithm: None,
            checksum_value: None,
            priority: None,
            start: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ManagerEvent {
    Updated(DownloadRecord),
    Progress {
        id: Uuid,
        progress: DownloadProgress,
    },
    Removed {
        id: Uuid,
    },
    Finished {
        id: Uuid,
        status: DownloadStatus,
    },
}

struct TaskEntry {
    cancel: CancellationToken,
    _join: JoinHandle<()>,
}

pub struct DownloadManager {
    config: ManagerConfig,
    db: Arc<Database>,
    events: broadcast::Sender<ManagerEvent>,
    tasks: Mutex<HashMap<Uuid, TaskEntry>>,
    live: Mutex<HashMap<Uuid, DownloadProgress>>,
    scheduler_active: Mutex<bool>,
    scheduler_started: Mutex<bool>,
    host_limiter: Arc<HostLimiter>,
}

impl DownloadManager {
    pub fn new(config: ManagerConfig) -> Result<Arc<Self>> {
        std::fs::create_dir_all(&config.data_dir)?;
        std::fs::create_dir_all(&config.download_dir)?;
        let db = Arc::new(Database::open(config.data_dir.join("rustget.db"))?);
        db.mark_interrupted()?;
        let per_host = usize::from(config.engine.max_connections_per_host);
        let (events, _) = broadcast::channel(1024);
        Ok(Arc::new(Self {
            config,
            db,
            events,
            tasks: Mutex::new(HashMap::new()),
            live: Mutex::new(HashMap::new()),
            scheduler_active: Mutex::new(false),
            scheduler_started: Mutex::new(false),
            host_limiter: Arc::new(HostLimiter::new(per_host)),
        }))
    }

    #[must_use]
    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }

    #[must_use]
    pub fn config(&self) -> &ManagerConfig {
        &self.config
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ManagerEvent> {
        self.events.subscribe()
    }

    pub fn list(&self, status: Option<DownloadStatus>) -> Result<Vec<DownloadRecord>> {
        Ok(self.db.list(status)?)
    }

    pub fn get(&self, id: Uuid) -> Result<DownloadRecord> {
        self.db.get(id)?.ok_or(ManagerError::NotFound(id))
    }

    #[must_use]
    pub fn live_progress(&self, id: Uuid) -> Option<DownloadProgress> {
        self.live.lock().get(&id).cloned()
    }

    /// Probe the URL, choose a collision-free destination and persist a queued
    /// row. Probing before insertion is what makes Content-Disposition names
    /// and the final save path visible to every client immediately.
    pub async fn add(self: &Arc<Self>, request: NewDownload) -> Result<DownloadRecord> {
        let url = Url::parse(&request.url)?;
        if !matches!(url.scheme(), "http" | "https" | "ftp") {
            return Err(ManagerError::InvalidState(format!(
                "unsupported URL scheme '{}' (ftps is not implemented yet)",
                url.scheme()
            )));
        }
        let is_ftp = url.scheme() == "ftp";
        let parsed_mirrors: Vec<Url> = if is_ftp {
            Vec::new()
        } else {
            request
                .mirrors
                .iter()
                .filter_map(|mirror| Url::parse(mirror).ok())
                .filter(|mirror| matches!(mirror.scheme(), "http" | "https"))
                .collect()
        };
        let (safe_name, final_url, total_size, etag, last_modified) = if is_ftp {
            let name =
                rustget_storage::filename_from_url(&url).unwrap_or_else(|| "download".into());
            (sanitize_filename(&name), url.to_string(), None, None, None)
        } else {
            let base_config = self.engine_for(request.connections, request.speed_limit);
            let options = RequestOptions {
                headers: request.headers.clone(),
                ..RequestOptions::default()
            };
            let downloader = Downloader::new(base_config, options)?
                .with_host_limiter(Arc::clone(&self.host_limiter));
            let mut probe = None;
            let mut last_error = None;
            for candidate in std::iter::once(&url).chain(parsed_mirrors.iter()) {
                match downloader.probe(candidate).await {
                    Ok(found) => {
                        probe = Some(found);
                        break;
                    }
                    Err(error) => last_error = Some(error),
                }
            }
            let Some(probe) = probe else {
                return Err(last_error
                    .map(ManagerError::Net)
                    .unwrap_or_else(|| ManagerError::Net(NetError::InvalidUrl(url.to_string()))));
            };
            (
                sanitize_filename(&probe.filename),
                probe.final_url.to_string(),
                probe.total_size,
                probe.validator.etag,
                probe.validator.last_modified,
            )
        };
        let extension = std::path::Path::new(&safe_name)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("");
        let automatic = self.db.category_for(extension)?;
        let requested_category = request
            .category
            .clone()
            .filter(|category| !category.eq_ignore_ascii_case("general"));
        let category = requested_category
            .clone()
            .or_else(|| automatic.as_ref().map(|category| category.name.clone()))
            .unwrap_or_else(|| "General".into());
        let category_directory = self
            .db
            .list_categories()?
            .into_iter()
            .find(|item| item.name == category)
            .map(|item| item.directory)
            .filter(|directory| !directory.as_os_str().is_empty());
        let directory = request
            .directory
            .clone()
            .or(category_directory)
            .unwrap_or_else(|| self.config.download_dir.clone());
        std::fs::create_dir_all(&directory)?;
        let destination = if let Some(path) = request.destination.clone() {
            if path.is_dir() {
                path.join(&safe_name)
            } else {
                path
            }
        } else {
            unique_destination(directory.join(&safe_name))
        };
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let filename = destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("download")
            .to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let record = DownloadRecord {
            id: Uuid::new_v4(),
            url: url.to_string(),
            mirrors: parsed_mirrors
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            final_url: Some(final_url),
            destination,
            filename,
            category,
            status: DownloadStatus::Queued,
            total_size,
            downloaded: 0,
            speed_bps: 0.0,
            connections: request
                .connections
                .unwrap_or(self.config.engine.connections)
                .clamp(1, self.config.engine.max_connections),
            speed_limit: request
                .speed_limit
                .or(self.config.engine.max_speed_bytes_per_sec),
            headers: request.headers,
            checksum_algorithm: request.checksum_algorithm,
            checksum_value: request.checksum_value,
            etag,
            last_modified,
            error: None,
            priority: request.priority.unwrap_or(0),
            created_at_ms: now,
            updated_at_ms: now,
            completed_at_ms: None,
        };
        self.db.insert(&record)?;
        let _ = self.events.send(ManagerEvent::Updated(record.clone()));
        if request.start {
            self.schedule();
        }
        Ok(record)
    }

    pub fn pause(&self, id: Uuid) -> Result<DownloadRecord> {
        let mut record = self.get(id)?;
        if record.status == DownloadStatus::Completed {
            return Err(ManagerError::InvalidState(
                "download already completed".into(),
            ));
        }
        if let Some(task) = self.tasks.lock().get(&id) {
            task.cancel.cancel();
        }
        record.status = DownloadStatus::Paused;
        record.speed_bps = 0.0;
        record.updated_at_ms = chrono::Utc::now().timestamp_millis();
        self.db.update(&record)?;
        let _ = self.events.send(ManagerEvent::Updated(record.clone()));
        Ok(record)
    }

    pub fn resume(self: &Arc<Self>, id: Uuid) -> Result<DownloadRecord> {
        let mut record = self.get(id)?;
        if record.status.is_active() {
            return Ok(record);
        }
        if record.status == DownloadStatus::Completed {
            return Err(ManagerError::InvalidState(
                "download already completed".into(),
            ));
        }
        record.status = DownloadStatus::Queued;
        record.error = None;
        record.updated_at_ms = chrono::Utc::now().timestamp_millis();
        self.db.update(&record)?;
        let _ = self.events.send(ManagerEvent::Updated(record.clone()));
        self.schedule();
        Ok(record)
    }

    pub fn set_priority(&self, id: Uuid, priority: i32) -> Result<DownloadRecord> {
        let mut record = self.get(id)?;
        record.priority = priority;
        record.updated_at_ms = chrono::Utc::now().timestamp_millis();
        self.db.update(&record)?;
        let _ = self.events.send(ManagerEvent::Updated(record.clone()));
        Ok(record)
    }

    pub fn pause_all(&self) -> usize {
        let items = self.db.list(None).unwrap_or_default();
        let mut count = 0;
        for item in items {
            if (item.status.is_active() || item.status == DownloadStatus::Queued)
                && self.pause(item.id).is_ok()
            {
                count += 1;
            }
        }
        count
    }

    pub fn resume_all(self: &Arc<Self>) -> usize {
        let items = self.db.list(None).unwrap_or_default();
        let mut count = 0;
        for item in items {
            if (item.status == DownloadStatus::Paused || item.status == DownloadStatus::Failed)
                && self.resume(item.id).is_ok()
            {
                count += 1;
            }
        }
        count
    }

    pub fn retry(self: &Arc<Self>, id: Uuid) -> Result<DownloadRecord> {
        let record = self.get(id)?;
        if record.status != DownloadStatus::Failed {
            return Err(ManagerError::InvalidState(
                "only failed downloads can be retried".into(),
            ));
        }
        self.resume(id)
    }

    pub fn remove(&self, id: Uuid, delete_files: bool) -> Result<()> {
        let record = self.get(id)?;
        if let Some(task) = self.tasks.lock().get(&id) {
            task.cancel.cancel();
        }
        if delete_files {
            let paths = DownloadPaths::new(&record.destination);
            for path in [&record.destination, &paths.part, &paths.journal] {
                if path.exists() {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        self.db.delete(id)?;
        self.live.lock().remove(&id);
        let _ = self.events.send(ManagerEvent::Removed { id });
        Ok(())
    }

    pub fn shutdown(&self) {
        for task in self.tasks.lock().values() {
            task.cancel.cancel();
        }
        let _ = self.db.mark_interrupted();
    }

    pub fn categories(&self) -> Result<Vec<crate::model::Category>> {
        Ok(self.db.list_categories()?)
    }

    pub fn schedules(&self) -> Result<Vec<crate::scheduler::Schedule>> {
        Ok(self.db.list_schedules()?)
    }

    pub fn add_schedule(&self, schedule: crate::scheduler::Schedule) -> Result<()> {
        self.db.insert_schedule(&schedule)?;
        Ok(())
    }

    pub fn remove_schedule(&self, id: Uuid) -> Result<()> {
        self.db.delete_schedule(id)?;
        Ok(())
    }

    /// Start the weekly window enforcement loop. Called by the daemon; one-off
    /// CLI downloads intentionally do not run a scheduler.
    pub fn start_scheduler(self: &Arc<Self>) {
        {
            let mut started = self.scheduler_started.lock();
            if *started {
                return;
            }
            *started = true;
        }
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(10));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                this.scheduler_tick();
            }
        });
    }

    fn scheduler_tick(self: &Arc<Self>) {
        let Ok(schedules) = self.db.list_schedules() else {
            return;
        };
        if schedules.is_empty() {
            return;
        }
        let now = chrono::Local::now();
        use chrono::Datelike;
        use chrono::Timelike;
        let day = u8::try_from(now.weekday().num_days_from_monday()).unwrap_or(0);
        let minute = u16::try_from(now.hour() * 60 + now.minute()).unwrap_or(0);
        let active = schedules
            .iter()
            .any(|schedule| schedule.active_at(day, minute));
        let previous = {
            let mut flag = self.scheduler_active.lock();
            let previous = *flag;
            *flag = active;
            previous
        };
        if active && !previous {
            tracing::info!("scheduler window opened");
            let items = self
                .db
                .list(Some(DownloadStatus::Paused))
                .unwrap_or_default();
            for mut item in items {
                if item.error.as_deref() == Some("paused by scheduler") {
                    item.error = None;
                    item.status = DownloadStatus::Queued;
                    let _ = self.db.update(&item);
                    let _ = self.events.send(ManagerEvent::Updated(item));
                }
            }
            self.schedule();
        } else if !active && previous {
            tracing::info!("scheduler window closed");
            let items = self
                .db
                .list(Some(DownloadStatus::Downloading))
                .unwrap_or_default();
            for item in items {
                let id = item.id;
                if let Ok(mut record) = self.pause(id) {
                    record.error = Some("paused by scheduler".into());
                    record.updated_at_ms = chrono::Utc::now().timestamp_millis();
                    let _ = self.db.update(&record);
                    let _ = self.events.send(ManagerEvent::Updated(record));
                }
            }
        }
    }

    /// Execute configured power actions only after every queued transfer has
    /// stopped. Power actions are deliberately opt-in at the platform layer.
    #[must_use]
    pub fn pending_power_action(&self) -> Option<crate::scheduler::PowerAction> {
        let schedules = self.db.list_schedules().ok()?;
        let active = schedules.into_iter().find(|schedule| {
            schedule.enabled && schedule.action != crate::scheduler::PowerAction::None
        })?;
        let busy = self.tasks.lock().len();
        (busy == 0).then_some(active.action)
    }

    fn schedule(self: &Arc<Self>) {
        loop {
            let active = self.tasks.lock().len();
            if active >= self.config.max_concurrent.max(1) {
                break;
            }
            let Some(record) = self.db.next_queued().ok().flatten() else {
                break;
            };
            if self.tasks.lock().contains_key(&record.id) {
                break;
            }
            self.spawn(record);
        }
    }

    fn spawn(self: &Arc<Self>, mut record: DownloadRecord) {
        let id = record.id;
        let was_paused = record.status == DownloadStatus::Paused;
        let engine = self.engine_for(Some(record.connections), record.speed_limit);
        let options = RequestOptions {
            headers: record.headers.clone(),
            ..RequestOptions::default()
        };
        let downloader = match Downloader::new(engine, options)
            .map(|downloader| downloader.with_host_limiter(Arc::clone(&self.host_limiter)))
        {
            Ok(downloader) => downloader,
            Err(error) => {
                record.status = DownloadStatus::Failed;
                record.error = Some(error.to_string());
                record.updated_at_ms = chrono::Utc::now().timestamp_millis();
                let _ = self.db.update(&record);
                let _ = self.events.send(ManagerEvent::Updated(record));
                return;
            }
        };
        let checksum = match (
            record.checksum_algorithm.clone(),
            record.checksum_value.clone(),
        ) {
            (Some(algorithm), Some(value)) => match ChecksumSpec::new(&algorithm, &value) {
                Ok(spec) => Some(spec),
                Err(error) => {
                    record.status = DownloadStatus::Failed;
                    record.error = Some(error.to_string());
                    let _ = self.db.update(&record);
                    return;
                }
            },
            _ => None,
        };
        let request = DownloadRequest {
            id: record.id,
            url: Url::parse(&record.url).unwrap_or_else(|_| Url::parse("http://invalid").unwrap()),
            mirrors: record
                .mirrors
                .iter()
                .filter_map(|mirror| Url::parse(mirror).ok())
                .collect(),
            destination: Some(record.destination.clone()),
            directory: self.config.download_dir.clone(),
            resume: was_paused || DownloadPaths::new(&record.destination).part.exists(),
            overwrite: false,
            checksum,
        };
        let (handle, tx) = downloader.handle(record.id);
        let cancel = handle.cancel.clone();
        let mut progress = handle.progress;
        let this = Arc::clone(self);
        let task = tokio::spawn(async move {
            record.status = DownloadStatus::Downloading;
            let _ = this.db.update(&record);
            let _ = this.events.send(ManagerEvent::Updated(record.clone()));
            let mut download = Box::pin(downloader.download(request, cancel, tx));
            let mut last_persist = Instant::now();
            let result = loop {
                tokio::select! {
                    result = &mut download => break result,
                    changed = progress.changed() => {
                        if changed.is_err() {
                            continue;
                        }
                        let snapshot = progress.borrow().clone();
                        apply_progress(&mut record, &snapshot);
                        if last_persist.elapsed() >= Duration::from_millis(250) {
                            let _ = this.db.update(&record);
                            let _ = this.events.send(ManagerEvent::Updated(record.clone()));
                            this.live.lock().insert(record.id, snapshot.clone());
                            let _ = this.events.send(ManagerEvent::Progress {
                                id: record.id,
                                progress: snapshot,
                            });
                            last_persist = Instant::now();
                        }
                    }
                }
            };
            let status = match result {
                Ok(summary) => {
                    record.filename = summary
                        .destination
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(&record.filename)
                        .to_string();
                    record.destination = summary.destination;
                    record.final_url = Some(summary.final_url);
                    record.total_size = Some(summary.total_size);
                    record.downloaded = summary.total_size;
                    record.speed_bps = 0.0;
                    record.status = DownloadStatus::Completed;
                    record.error = None;
                    record.completed_at_ms = Some(chrono::Utc::now().timestamp_millis());
                    DownloadStatus::Completed
                }
                Err(NetError::Cancelled) => {
                    record.status = DownloadStatus::Paused;
                    record.speed_bps = 0.0;
                    DownloadStatus::Paused
                }
                Err(error) => {
                    record.status = DownloadStatus::Failed;
                    record.speed_bps = 0.0;
                    record.error = Some(error.to_string());
                    DownloadStatus::Failed
                }
            };
            record.updated_at_ms = chrono::Utc::now().timestamp_millis();
            let _ = this.db.update(&record);
            this.live.lock().remove(&record.id);
            let _ = this.events.send(ManagerEvent::Updated(record.clone()));
            let _ = this.events.send(ManagerEvent::Finished {
                id: record.id,
                status,
            });
            this.tasks.lock().remove(&record.id);
            this.schedule();
        });
        self.tasks.lock().insert(
            id,
            TaskEntry {
                cancel: handle.cancel,
                _join: task,
            },
        );
    }

    fn engine_for(&self, connections: Option<u8>, speed_limit: Option<u64>) -> EngineConfig {
        let mut engine = self.config.engine.clone();
        if let Some(connections) = connections {
            engine.connections = connections.clamp(1, engine.max_connections);
        }
        engine.max_speed_bytes_per_sec = speed_limit.or(engine.max_speed_bytes_per_sec);
        engine
    }
}

fn apply_progress(record: &mut DownloadRecord, snapshot: &DownloadProgress) {
    record.status = match snapshot.state {
        DownloadState::Queued => DownloadStatus::Queued,
        DownloadState::Probing => DownloadStatus::Probing,
        DownloadState::Downloading => DownloadStatus::Downloading,
        DownloadState::Finalizing => DownloadStatus::Finalizing,
        DownloadState::Complete => DownloadStatus::Completed,
        DownloadState::Paused => DownloadStatus::Paused,
        DownloadState::Failed => DownloadStatus::Failed,
    };
    record.total_size = snapshot.total_size.or(record.total_size);
    record.downloaded = snapshot.downloaded;
    record.speed_bps = snapshot.speed_bps;
    if let Some(url) = &snapshot.final_url {
        record.final_url = Some(url.clone());
    }
    record.updated_at_ms = chrono::Utc::now().timestamp_millis();
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustget_testkit::{TestServer, TestkitConfig, expected_content};

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn manager_persists_and_completes_download() {
        let server = TestServer::start(TestkitConfig::single(512 * 1024))
            .await
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let manager = DownloadManager::new(ManagerConfig {
            data_dir: dir.path().join("data"),
            download_dir: dir.path().join("downloads"),
            engine: EngineConfig::default(),
            max_concurrent: 2,
        })
        .unwrap();
        let mut events = manager.subscribe();
        let record = manager
            .add(NewDownload {
                url: server.file_url("file.bin"),
                mirrors: Vec::new(),
                ..NewDownload::default()
            })
            .await
            .unwrap();
        assert_eq!(record.status, DownloadStatus::Queued);
        let completed = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                if let Ok(ManagerEvent::Finished { id, status }) = events.recv().await {
                    if id == record.id {
                        break status;
                    }
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(completed, DownloadStatus::Completed);
        let stored = manager.get(record.id).unwrap();
        assert_eq!(stored.status, DownloadStatus::Completed);
        let bytes = std::fs::read(&stored.destination).unwrap();
        assert_eq!(bytes, expected_content(0x5EED_1234_ABCD_9876, 512 * 1024));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn auto_routes_category_directory_by_extension() {
        let server = TestServer::start(
            rustget_testkit::TestkitConfig::default()
                .with_file(rustget_testkit::TestFile::new("song.mp3", 64 * 1024)),
        )
        .await
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("music");
        let manager = DownloadManager::new(ManagerConfig {
            data_dir: dir.path().join("data"),
            download_dir: dir.path().join("downloads"),
            engine: EngineConfig::default(),
            max_concurrent: 1,
        })
        .unwrap();
        manager
            .database()
            .set_category_directory("Music", music.to_str().unwrap())
            .unwrap();
        let record = manager
            .add(NewDownload {
                url: server.file_url("song.mp3"),
                mirrors: Vec::new(),
                start: false,
                ..NewDownload::default()
            })
            .await
            .unwrap();
        assert_eq!(record.category, "Music");
        assert_eq!(record.destination.parent(), Some(music.as_path()));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn manager_routes_hls_and_updates_output_extension() {
        use rustget_testkit::{TestFile, TestkitConfig};
        let server = TestServer::start(
            TestkitConfig::default()
                .with_file(TestFile::inline(
                    "stream.m3u8",
                    "application/vnd.apple.mpegurl",
                    b"#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\na.m4s\n#EXTINF:1,\nb.m4s\n#EXT-X-ENDLIST\n",
                ))
                .with_file(TestFile::inline("a.m4s", "video/iso.segment", b"AAA"))
                .with_file(TestFile::inline("b.m4s", "video/iso.segment", b"BBB")),
        )
        .await
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let manager = DownloadManager::new(ManagerConfig {
            data_dir: dir.path().join("data"),
            download_dir: dir.path().join("downloads"),
            engine: EngineConfig::default(),
            max_concurrent: 1,
        })
        .unwrap();
        let mut events = manager.subscribe();
        let record = manager
            .add(NewDownload {
                url: server.file_url("stream.m3u8"),
                mirrors: Vec::new(),
                ..NewDownload::default()
            })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if let Ok(ManagerEvent::Finished { id, status }) = events.recv().await {
                    if id == record.id {
                        assert_eq!(status, DownloadStatus::Completed);
                        break;
                    }
                }
            }
        })
        .await
        .unwrap();
        let stored = manager.get(record.id).unwrap();
        assert!(stored.destination.ends_with("stream.ts"));
        assert_eq!(std::fs::read(&stored.destination).unwrap(), b"AAABBB");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn manager_uses_mirror_when_primary_is_down() {
        let mirror = TestServer::start(TestkitConfig::single(256 * 1024))
            .await
            .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let manager = DownloadManager::new(ManagerConfig {
            data_dir: temp.path().join("data"),
            download_dir: temp.path().join("downloads"),
            engine: EngineConfig::default(),
            max_concurrent: 1,
        })
        .unwrap();
        let mut events = manager.subscribe();
        let record = manager
            .add(NewDownload {
                url: "http://127.0.0.1:1/down".into(),
                mirrors: vec![mirror.file_url("file.bin")],
                ..NewDownload::default()
            })
            .await
            .unwrap();
        assert_eq!(record.mirrors.len(), 1);
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if let Ok(ManagerEvent::Finished { id, status }) = events.recv().await {
                    if id == record.id {
                        assert_eq!(status, DownloadStatus::Completed);
                        break;
                    }
                }
            }
        })
        .await
        .unwrap();
        let stored = manager.get(record.id).unwrap();
        assert_eq!(
            std::fs::read(&stored.destination).unwrap(),
            expected_content(0x5EED_1234_ABCD_9876, 256 * 1024)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn manager_downloads_ftp_without_http_probe() {
        use rustget_testkit::{FtpFixtureConfig, FtpTestServer};
        let content = expected_content(0x5EED_1234_ABCD_9876, 256 * 1024);
        let server = FtpTestServer::start(FtpFixtureConfig::new(content.clone()))
            .await
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let manager = DownloadManager::new(ManagerConfig {
            data_dir: dir.path().join("data"),
            download_dir: dir.path().join("downloads"),
            engine: EngineConfig::default(),
            max_concurrent: 1,
        })
        .unwrap();
        let mut events = manager.subscribe();
        let record = manager
            .add(NewDownload {
                url: server.url(),
                ..NewDownload::default()
            })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if let Ok(ManagerEvent::Finished { id, status }) = events.recv().await {
                    if id == record.id {
                        assert_eq!(status, DownloadStatus::Completed);
                        break;
                    }
                }
            }
        })
        .await
        .unwrap();
        let stored = manager.get(record.id).unwrap();
        assert_eq!(std::fs::read(&stored.destination).unwrap(), content);
        server.shutdown().await;
    }

    #[test]
    fn progress_is_mapped_to_record() {
        let mut record = DownloadRecord {
            id: Uuid::new_v4(),
            url: String::new(),
            mirrors: Vec::new(),
            final_url: None,
            destination: PathBuf::new(),
            filename: String::new(),
            category: String::new(),
            status: DownloadStatus::Queued,
            total_size: None,
            downloaded: 0,
            speed_bps: 0.0,
            connections: 8,
            speed_limit: None,
            headers: Vec::new(),
            checksum_algorithm: None,
            checksum_value: None,
            etag: None,
            last_modified: None,
            error: None,
            priority: 0,
            created_at_ms: 0,
            updated_at_ms: 0,
            completed_at_ms: None,
        };
        let snapshot = DownloadProgress {
            task_id: record.id,
            state: DownloadState::Downloading,
            total_size: Some(100),
            downloaded: 25,
            speed_bps: 10.0,
            eta_secs: Some(7),
            active_connections: 2,
            segments: Vec::new(),
            final_url: Some("https://example.test/f".into()),
            filename: Some("f".into()),
            error: None,
            unit_total: None,
            unit_completed: 0,
        };
        apply_progress(&mut record, &snapshot);
        assert_eq!(record.status, DownloadStatus::Downloading);
        assert_eq!(record.downloaded, 25);
        assert_eq!(record.progress_ratio(), 0.25);
    }
}
