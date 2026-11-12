use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use rustget_core::{
    Assignment, ByteRange, EngineConfig, RetryPolicy, SegmentId, SegmentLedger, SegmentState,
    WorkerId,
};
use rustget_storage::{
    DownloadPaths, Journal, JournalState, JournalStore, PartFile, PreallocateMode,
    sanitize_filename, unique_destination,
};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

use crate::checksum::{ChecksumSpec, verify_checksum};
use crate::client::{NetClient, RequestOptions};
use crate::error::{NetError, Result};
use crate::probe::{FileProbe, Validator, parse_content_range};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Queued,
    Probing,
    Downloading,
    Finalizing,
    Complete,
    Paused,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentProgress {
    pub id: SegmentId,
    pub start: u64,
    pub end: u64,
    pub written: u64,
    pub state: SegmentState,
    pub attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub task_id: Uuid,
    pub state: DownloadState,
    pub total_size: Option<u64>,
    pub downloaded: u64,
    pub speed_bps: f64,
    pub eta_secs: Option<u64>,
    pub active_connections: usize,
    pub segments: Vec<SegmentProgress>,
    pub final_url: Option<String>,
    pub filename: Option<String>,
    pub error: Option<String>,
    /// For segmented media (HLS/DASH), progress can be expressed in media
    /// segments while the total byte size is still unknown.
    #[serde(default)]
    pub unit_total: Option<u64>,
    #[serde(default)]
    pub unit_completed: u64,
}

impl DownloadProgress {
    #[must_use]
    pub fn queued(task_id: Uuid) -> Self {
        Self {
            task_id,
            state: DownloadState::Queued,
            total_size: None,
            downloaded: 0,
            speed_bps: 0.0,
            eta_secs: None,
            active_connections: 0,
            segments: Vec::new(),
            final_url: None,
            filename: None,
            error: None,
            unit_total: None,
            unit_completed: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DownloadRequest {
    pub id: Uuid,
    pub url: Url,
    /// Alternate URLs expected to expose the same bytes. Used for failover.
    pub mirrors: Vec<Url>,
    /// Explicit file path, or `None` to derive a sanitized name in `directory`.
    pub destination: Option<PathBuf>,
    pub directory: PathBuf,
    /// Reuse an existing journal and `.part` file.
    pub resume: bool,
    pub overwrite: bool,
    pub checksum: Option<ChecksumSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadSummary {
    pub id: Uuid,
    pub destination: PathBuf,
    pub final_url: String,
    pub total_size: u64,
    pub downloaded: u64,
    pub elapsed_ms: u64,
    pub segments: usize,
    pub resumed: bool,
    pub checksum_verified: bool,
    pub average_speed_bps: f64,
}

#[derive(Debug, Clone)]
pub struct ProgressHandle {
    pub progress: watch::Receiver<DownloadProgress>,
    pub cancel: CancellationToken,
}

#[derive(Debug, Clone)]
pub struct Downloader {
    client: Arc<NetClient>,
    config: EngineConfig,
    host_limiter: Option<Arc<crate::host::HostLimiter>>,
}

impl Downloader {
    pub fn new(config: EngineConfig, options: RequestOptions) -> Result<Self> {
        config
            .validate()
            .map_err(|error| NetError::Config(error.to_string()))?;
        let client = Arc::new(NetClient::new(&config, options)?);
        Ok(Self {
            client,
            config,
            host_limiter: None,
        })
    }

    #[must_use]
    pub fn with_host_limiter(mut self, limiter: Arc<crate::host::HostLimiter>) -> Self {
        self.host_limiter = Some(limiter);
        self
    }

    #[must_use]
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Probe a URL without starting a transfer. The manager uses this to
    /// resolve Content-Disposition names before inserting a download row.
    pub async fn probe(&self, url: &Url) -> Result<FileProbe> {
        self.client.probe(url, &self.config).await
    }

    /// Create a progress receiver and cancellation token for one download.
    #[must_use]
    pub fn handle(&self, id: Uuid) -> (ProgressHandle, watch::Sender<DownloadProgress>) {
        let (tx, rx) = watch::channel(DownloadProgress::queued(id));
        (
            ProgressHandle {
                progress: rx,
                cancel: CancellationToken::new(),
            },
            tx,
        )
    }

    pub async fn download(
        &self,
        request: DownloadRequest,
        cancel: CancellationToken,
        progress: watch::Sender<DownloadProgress>,
    ) -> Result<DownloadSummary> {
        let started = Instant::now();
        let mut tracker = Tracker::new(progress, request.id);
        if request.url.scheme() == "ftp" {
            return self
                .download_ftp(&request, cancel, &mut tracker, started)
                .await;
        }
        if request.url.scheme() == "ftps" {
            return Err(NetError::Protocol(
                "FTPS is not supported yet; use plain ftp:// for now".into(),
            ));
        }
        if is_hls_url(&request.url) {
            return self
                .download_hls(&request, cancel, &mut tracker, started)
                .await;
        }
        if is_mpd_url(&request.url) {
            return self
                .download_dash(&request, cancel, &mut tracker, started)
                .await;
        }
        tracker.emit(
            DownloadState::Probing,
            None,
            0,
            0,
            Vec::new(),
            None,
            None,
            None,
        );
        let mut request = request;
        let mut candidates = vec![request.url.clone()];
        candidates.extend(request.mirrors.iter().cloned());
        candidates.dedup();
        let mut probe = None;
        let mut last_error = None;
        for candidate in &candidates {
            match self.client.probe(candidate, &self.config).await {
                Ok(found) => {
                    request.url = candidate.clone();
                    probe = Some(found);
                    break;
                }
                Err(error) => last_error = Some(error),
            }
        }
        let probe = match probe {
            Some(probe) => probe,
            None => {
                return Err(
                    last_error.unwrap_or_else(|| NetError::Protocol("no usable URL".into()))
                );
            }
        };
        let destination = resolve_destination(&request, &probe)?;
        let paths = DownloadPaths::new(&destination);
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tracker.emit(
            DownloadState::Downloading,
            probe.total_size,
            0,
            0,
            Vec::new(),
            Some(probe.final_url.as_str()),
            Some(
                destination
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("download")
                    .to_string(),
            ),
            None,
        );

        if probe.total_size == Some(0) {
            let summary = finish_empty(&request, &probe, &paths, started)?;
            tracker.emit(
                DownloadState::Complete,
                Some(0),
                0,
                0,
                Vec::new(),
                Some(probe.final_url.as_str()),
                None,
                None,
            );
            return Ok(summary);
        }

        let ranged_possible = probe.range_supported
            && probe.total_size.is_some()
            && probe.total_size.unwrap_or(0) >= self.config.min_segment_size;

        if ranged_possible {
            let existing = if request.resume {
                Journal::load(&paths.journal)
            } else {
                None
            };
            match self
                .run_ranged(
                    &request,
                    &probe,
                    &paths,
                    existing,
                    cancel.clone(),
                    &mut tracker,
                    started,
                )
                .await
            {
                Ok(summary) => return Ok(summary),
                Err(NetError::RangeUnsupported) => {
                    tracing::warn!(
                        url = %request.url,
                        "server ignored a ranged request after probe; restarting single-stream"
                    );
                    reset_partial(&paths)?;
                }
                Err(error @ NetError::RemoteChanged(_)) if self.config.restart_on_change => {
                    tracing::warn!(url = %request.url, %error, "remote changed; restarting");
                    reset_partial(&paths)?;
                }
                Err(error) => return Err(error),
            }
        }

        let summary = self
            .run_single(&request, &probe, &paths, cancel, &mut tracker, started)
            .await?;
        Ok(summary)
    }

    async fn download_ftp(
        &self,
        request: &DownloadRequest,
        cancel: CancellationToken,
        tracker: &mut Tracker,
        started: Instant,
    ) -> Result<DownloadSummary> {
        use suppaftp::types::FileType;
        use suppaftp::{FtpError, FtpStream};

        let host = request
            .url
            .host_str()
            .ok_or_else(|| NetError::InvalidUrl("FTP URL has no host".into()))?
            .to_string();
        let port = request.url.port().unwrap_or(21);
        let remote_path = percent_encoding::percent_decode_str(request.url.path())
            .decode_utf8_lossy()
            .to_string();
        let (user, password) = if request.url.username().is_empty() {
            self.client
                .basic_auth()
                .cloned()
                .unwrap_or_else(|| ("anonymous".into(), String::new()))
        } else {
            (
                request.url.username().to_string(),
                request.url.password().unwrap_or("").to_string(),
            )
        };
        let destination = ftp_destination(request)?;
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let paths = DownloadPaths::new(&destination);
        if paths.destination.exists() && !request.overwrite {
            return Err(NetError::DestinationExists(
                paths.destination.display().to_string(),
            ));
        }
        tracker.emit(
            DownloadState::Probing,
            None,
            0,
            1,
            Vec::new(),
            Some(request.url.as_str()),
            None,
            None,
        );
        let total = Arc::new(AtomicU64::new(0));
        let downloaded = Arc::new(AtomicU64::new(0));
        let total_for_thread = Arc::clone(&total);
        let downloaded_for_thread = Arc::clone(&downloaded);
        let part_path = paths.part.clone();
        let cancel_thread = cancel.clone();
        let mut blocking = tokio::task::spawn_blocking(move || {
            let mut ftp = FtpStream::connect((host.as_str(), port)).map_err(ftp_error)?;
            ftp.login(&user, &password).map_err(ftp_error)?;
            ftp.transfer_type(FileType::Binary).map_err(ftp_error)?;
            let size = ftp.size(&remote_path).ok();
            if let Some(size) = size {
                total_for_thread.store(size as u64, Ordering::Release);
            }
            let existing = std::fs::metadata(&part_path).map_or(0, |meta| meta.len());
            let offset = if size.is_some_and(|size| existing as usize > size) {
                0
            } else {
                existing
            };
            downloaded_for_thread.store(offset, Ordering::Release);
            let part = if let Some(size) = size {
                PartFile::open(&part_path, size as u64, PreallocateMode::Sparse)?
            } else {
                PartFile::open_unbounded(&part_path)?
            };
            if offset > 0 {
                ftp.resume_transfer(offset as usize).map_err(ftp_error)?;
            }
            let mut written = offset;
            if size.is_none_or(|size| written < size as u64) {
                let result = ftp.retr(&remote_path, |stream| {
                    let mut buffer = [0_u8; 64 * 1024];
                    loop {
                        if cancel_thread.is_cancelled() {
                            return Err(FtpError::ConnectionError(std::io::Error::new(
                                std::io::ErrorKind::Interrupted,
                                "download cancelled",
                            )));
                        }
                        let read = stream
                            .read(&mut buffer)
                            .map_err(FtpError::ConnectionError)?;
                        if read == 0 {
                            break;
                        }
                        part.write_at(written, &buffer[..read])
                            .map_err(FtpError::ConnectionError)?;
                        written += read as u64;
                        downloaded_for_thread.store(written, Ordering::Release);
                    }
                    Ok(())
                });
                if let Err(error) = result {
                    if cancel_thread.is_cancelled() {
                        return Err(NetError::Cancelled);
                    }
                    return Err(ftp_error(error));
                }
            }
            if let Some(size) = size {
                if written != size as u64 {
                    return Err(NetError::ShortRead(size as u64 - written));
                }
            }
            let _ = ftp.quit();
            part.set_len(written)?;
            part.sync_all()?;
            Ok::<u64, NetError>(written)
        });
        let mut ticker = tokio::time::interval(Duration::from_millis(200));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let written = loop {
            tokio::select! {
                result = &mut blocking => {
                    let written = result
                        .map_err(|error| NetError::Protocol(format!("FTP worker failed: {error}")))??;
                    break written;
                }
                _ = ticker.tick() => {
                    let known = total.load(Ordering::Acquire);
                    tracker.emit(
                        DownloadState::Downloading,
                        (known > 0).then_some(known),
                        downloaded.load(Ordering::Acquire),
                        1,
                        vec![SegmentProgress {
                            id: SegmentId(0),
                            start: 0,
                            end: known.saturating_sub(1),
                            written: downloaded.load(Ordering::Acquire),
                            state: SegmentState::Active,
                            attempts: 1,
                        }],
                        Some(request.url.as_str()),
                        None,
                        None,
                    );
                }
                _ = cancel.cancelled() => {
                    let _ = (&mut blocking).await;
                    return Err(NetError::Cancelled);
                }
            }
        };
        if let Some(spec) = &request.checksum {
            verify_checksum(&paths.part, spec).await?;
        }
        std::fs::rename(&paths.part, &paths.destination)?;
        remove_journal(&paths);
        let summary = DownloadSummary {
            id: request.id,
            destination: paths.destination.clone(),
            final_url: request.url.to_string(),
            total_size: written,
            downloaded: written,
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            segments: 1,
            resumed: false,
            checksum_verified: request.checksum.is_some(),
            average_speed_bps: if started.elapsed().as_secs_f64() > 0.0 {
                written as f64 / started.elapsed().as_secs_f64()
            } else {
                0.0
            },
        };
        tracker.emit(
            DownloadState::Complete,
            Some(written),
            written,
            0,
            Vec::new(),
            Some(request.url.as_str()),
            None,
            None,
        );
        Ok(summary)
    }

    async fn download_hls(
        &self,
        request: &DownloadRequest,
        cancel: CancellationToken,
        tracker: &mut Tracker,
        started: Instant,
    ) -> Result<DownloadSummary> {
        use rustget_media::HlsPlaylist;

        tracker.emit(
            DownloadState::Probing,
            None,
            0,
            1,
            Vec::new(),
            Some(request.url.as_str()),
            None,
            None,
        );
        let mut playlist_url = request.url.clone();
        let playlist = self.fetch_hls_playlist(&playlist_url, &cancel).await?;
        let media = match playlist {
            HlsPlaylist::Master(variants) => {
                let variant = variants
                    .into_iter()
                    .max_by_key(|variant| variant.bandwidth)
                    .ok_or(NetError::Protocol("HLS master has no variants".into()))?;
                playlist_url = Url::parse(&variant.url)
                    .map_err(|error| NetError::InvalidUrl(error.to_string()))?;
                match self.fetch_hls_playlist(&playlist_url, &cancel).await? {
                    HlsPlaylist::Media(media) => media,
                    HlsPlaylist::Master(_) => {
                        return Err(NetError::Protocol(
                            "nested HLS master playlists are not supported".into(),
                        ));
                    }
                }
            }
            HlsPlaylist::Media(media) => media,
        };
        if media.encrypted {
            return Err(NetError::Protocol(
                "encrypted HLS playlists are not supported yet".into(),
            ));
        }
        tracker.set_units(media.segments.len() as u64, 0);
        let destination = hls_destination(request, &request.url)?;
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let paths = DownloadPaths::new(&destination);
        if paths.destination.exists() && !request.overwrite {
            return Err(NetError::DestinationExists(
                paths.destination.display().to_string(),
            ));
        }
        let part = PartFile::open_unbounded(&paths.part)?;
        let limiter =
            rustget_core::RateLimiter::new(self.config.max_speed_bytes_per_sec.unwrap_or(0));
        let retry = RetryPolicy::from_engine(&self.config);
        let mut cursor = 0_u64;
        let mut completed = 0_usize;
        let mut pseudo_segments = Vec::new();
        if let Some(init) = &media.init_map {
            let bytes = self
                .fetch_media_bytes(init, &cancel, &retry, &limiter)
                .await?;
            part.write_at(cursor, &bytes)?;
            cursor += bytes.len() as u64;
        }
        tracker.emit(
            DownloadState::Downloading,
            None,
            cursor,
            1,
            Vec::new(),
            Some(playlist_url.as_str()),
            None,
            None,
        );
        for segment in &media.segments {
            let bytes = self
                .fetch_media_bytes(&segment.url, &cancel, &retry, &limiter)
                .await?;
            part.write_at(cursor, &bytes)?;
            cursor += bytes.len() as u64;
            completed += 1;
            tracker.set_units(media.segments.len() as u64, completed as u64);
            pseudo_segments.push(SegmentProgress {
                id: SegmentId(completed as u64 - 1),
                start: completed as u64 - 1,
                end: completed as u64 - 1,
                written: completed as u64,
                state: SegmentState::Complete,
                attempts: 1,
            });
            if pseudo_segments.len() > 256 {
                pseudo_segments.remove(0);
            }
            if completed % 10 == 0 || completed == media.segments.len() {
                tracker.emit(
                    DownloadState::Downloading,
                    None,
                    cursor,
                    1,
                    pseudo_segments.clone(),
                    Some(playlist_url.as_str()),
                    None,
                    None,
                );
            }
        }
        part.set_len(cursor)?;
        part.sync_all()?;
        if let Some(spec) = &request.checksum {
            verify_checksum(&paths.part, spec).await?;
        }
        std::fs::rename(&paths.part, &paths.destination)?;
        remove_journal(&paths);
        let summary = DownloadSummary {
            id: request.id,
            destination: paths.destination.clone(),
            final_url: playlist_url.to_string(),
            total_size: cursor,
            downloaded: cursor,
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            segments: media.segments.len(),
            resumed: false,
            checksum_verified: request.checksum.is_some(),
            average_speed_bps: if started.elapsed().as_secs_f64() > 0.0 {
                cursor as f64 / started.elapsed().as_secs_f64()
            } else {
                0.0
            },
        };
        tracker.emit(
            DownloadState::Complete,
            Some(cursor),
            cursor,
            0,
            pseudo_segments,
            Some(playlist_url.as_str()),
            None,
            None,
        );
        Ok(summary)
    }

    async fn download_dash(
        &self,
        request: &DownloadRequest,
        cancel: CancellationToken,
        tracker: &mut Tracker,
        started: Instant,
    ) -> Result<DownloadSummary> {
        tracker.emit(
            DownloadState::Probing,
            None,
            0,
            1,
            Vec::new(),
            Some(request.url.as_str()),
            None,
            None,
        );
        let text = self
            .fetch_media_text(&request.url, &cancel, "DASH manifest", 16 * 1024 * 1024)
            .await?;
        let manifest = rustget_media::parse_mpd(&text, &request.url)
            .map_err(|error| NetError::Protocol(error.to_string()))?;
        let representation = manifest
            .representations
            .into_iter()
            .max_by_key(|representation| representation.bandwidth)
            .ok_or(NetError::Protocol(
                "DASH manifest has no representations".into(),
            ))?;
        if representation.drm {
            return Err(NetError::Protocol(
                "DRM-protected DASH representations are not supported".into(),
            ));
        }
        let count = representation.segment_count().ok_or_else(|| {
            NetError::Protocol(
                "DASH SegmentTimeline/live manifests are not supported yet; \
                 a duration-based SegmentTemplate is required"
                    .into(),
            )
        })?;
        if count > 100_000 {
            return Err(NetError::Protocol(format!(
                "DASH representation has {count} segments; refusing a live/huge manifest"
            )));
        }
        tracker.set_units(count, 0);
        let destination = dash_destination(request)?;
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let paths = DownloadPaths::new(&destination);
        if paths.destination.exists() && !request.overwrite {
            return Err(NetError::DestinationExists(
                paths.destination.display().to_string(),
            ));
        }
        let part = PartFile::open_unbounded(&paths.part)?;
        let limiter =
            rustget_core::RateLimiter::new(self.config.max_speed_bytes_per_sec.unwrap_or(0));
        let retry = RetryPolicy::from_engine(&self.config);
        let base = Url::parse(&representation.base_url)
            .map_err(|error| NetError::InvalidUrl(error.to_string()))?;
        let mut cursor = 0_u64;
        let mut pseudo_segments = Vec::new();
        if let Some(init) = &representation.initialization {
            let bytes = self
                .fetch_media_bytes(init, &cancel, &retry, &limiter)
                .await?;
            part.write_at(cursor, &bytes)?;
            cursor += bytes.len() as u64;
        }
        tracker.emit(
            DownloadState::Downloading,
            None,
            cursor,
            1,
            Vec::new(),
            Some(request.url.as_str()),
            None,
            None,
        );
        for index in 0..count {
            let number = representation.start_number + index;
            let relative = representation
                .segment_url(number)
                .ok_or_else(|| NetError::Protocol(format!("cannot build DASH segment {number}")))?;
            let url = base
                .join(&relative)
                .map_err(|error| NetError::InvalidUrl(error.to_string()))?;
            let bytes = self
                .fetch_media_bytes(url.as_str(), &cancel, &retry, &limiter)
                .await?;
            part.write_at(cursor, &bytes)?;
            cursor += bytes.len() as u64;
            tracker.set_units(count, index + 1);
            pseudo_segments.push(SegmentProgress {
                id: SegmentId(index),
                start: index,
                end: index,
                written: index + 1,
                state: SegmentState::Complete,
                attempts: 1,
            });
            if pseudo_segments.len() > 256 {
                pseudo_segments.remove(0);
            }
            if index % 10 == 0 || index + 1 == count {
                tracker.emit(
                    DownloadState::Downloading,
                    None,
                    cursor,
                    1,
                    pseudo_segments.clone(),
                    Some(request.url.as_str()),
                    None,
                    None,
                );
            }
        }
        part.set_len(cursor)?;
        part.sync_all()?;
        if let Some(spec) = &request.checksum {
            verify_checksum(&paths.part, spec).await?;
        }
        std::fs::rename(&paths.part, &paths.destination)?;
        remove_journal(&paths);
        let summary = DownloadSummary {
            id: request.id,
            destination: paths.destination.clone(),
            final_url: request.url.to_string(),
            total_size: cursor,
            downloaded: cursor,
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            segments: count as usize,
            resumed: false,
            checksum_verified: request.checksum.is_some(),
            average_speed_bps: if started.elapsed().as_secs_f64() > 0.0 {
                cursor as f64 / started.elapsed().as_secs_f64()
            } else {
                0.0
            },
        };
        tracker.emit(
            DownloadState::Complete,
            Some(cursor),
            cursor,
            0,
            pseudo_segments,
            Some(request.url.as_str()),
            None,
            None,
        );
        Ok(summary)
    }

    async fn fetch_hls_playlist(
        &self,
        url: &Url,
        cancel: &CancellationToken,
    ) -> Result<rustget_media::HlsPlaylist> {
        let text = self
            .fetch_media_text(url, cancel, "HLS playlist", 8 * 1024 * 1024)
            .await?;
        rustget_media::parse_hls(&text, url).map_err(|error| NetError::Protocol(error.to_string()))
    }

    async fn fetch_media_text(
        &self,
        url: &Url,
        cancel: &CancellationToken,
        label: &str,
        max_bytes: usize,
    ) -> Result<String> {
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(NetError::Cancelled),
            response = self
                .client
                .apply_common(self.client.client().get(url.clone()))?
                .send() => response?,
        };
        let status = response.status();
        if !status.is_success() {
            return Err(NetError::status(
                status.as_u16(),
                url.as_str(),
                status
                    .canonical_reason()
                    .unwrap_or("manifest request failed"),
            ));
        }
        let body = response.bytes().await?;
        if body.len() > max_bytes {
            return Err(NetError::Protocol(format!(
                "{label} exceeds {max_bytes} bytes"
            )));
        }
        String::from_utf8(body.to_vec())
            .map_err(|_| NetError::Protocol(format!("{label} is not UTF-8")))
    }

    async fn fetch_media_bytes(
        &self,
        url: &str,
        cancel: &CancellationToken,
        retry: &RetryPolicy,
        limiter: &rustget_core::RateLimiter,
    ) -> Result<Vec<u8>> {
        let parsed = Url::parse(url).map_err(|error| NetError::InvalidUrl(error.to_string()))?;
        let mut attempt = 0_u32;
        loop {
            let result: Result<Vec<u8>> = async {
                let response = tokio::select! {
                    _ = cancel.cancelled() => return Err(NetError::Cancelled),
                    response = self
                        .client
                        .apply_common(self.client.client().get(parsed.clone()))?
                        .send() => response?,
                };
                let status = response.status();
                if !status.is_success() {
                    return Err(NetError::status(
                        status.as_u16(),
                        parsed.as_str(),
                        status
                            .canonical_reason()
                            .unwrap_or("segment request failed"),
                    ));
                }
                let bytes = response.bytes().await?;
                if bytes.len() > 512 * 1024 * 1024 {
                    return Err(NetError::Protocol("HLS segment exceeds 512 MiB".into()));
                }
                tokio::select! {
                    _ = cancel.cancelled() => return Err(NetError::Cancelled),
                    _ = limiter.acquire(bytes.len() as u64) => {}
                }
                Ok(bytes.to_vec())
            }
            .await;
            match result {
                Ok(bytes) => return Ok(bytes),
                Err(NetError::Cancelled) => return Err(NetError::Cancelled),
                Err(error) => {
                    let class = error.class();
                    if !retry.should_retry(class, attempt + 1) {
                        return Err(error);
                    }
                    attempt += 1;
                    let delay = retry.delay(class, attempt);
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(NetError::Cancelled),
                        _ = tokio::time::sleep(delay) => {}
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_ranged(
        &self,
        request: &DownloadRequest,
        probe: &FileProbe,
        paths: &DownloadPaths,
        existing: Option<JournalState>,
        cancel: CancellationToken,
        tracker: &mut Tracker,
        started: Instant,
    ) -> Result<DownloadSummary> {
        let total = probe
            .total_size
            .ok_or(NetError::Protocol("ranged download without size".into()))?;
        let mut resumed = false;
        let mut ledger = if let Some(state) = &existing {
            if state.total_size != total {
                return Err(NetError::RemoteChanged(format!(
                    "size changed from {} to {total}",
                    state.total_size
                )));
            }
            if self.config.validate_on_resume {
                let stored = Validator {
                    etag: state.etag.clone(),
                    last_modified: state.last_modified.clone(),
                };
                if !stored.is_empty() && !stored.matches(&probe.validator) {
                    return Err(NetError::RemoteChanged(
                        "ETag/Last-Modified changed while paused".into(),
                    ));
                }
            }
            resumed = true;
            SegmentLedger::from_snapshot(state.segments.clone(), self.config.clone())
                .map_err(|error| NetError::Protocol(error.to_string()))?
        } else {
            if paths.destination.exists() && !request.overwrite {
                return Err(NetError::DestinationExists(
                    paths.destination.display().to_string(),
                ));
            }
            SegmentLedger::new(total, self.config.clone())
                .map_err(|error| NetError::Protocol(error.to_string()))?
        };

        if existing.as_ref().is_some_and(|state| state.complete) {
            if paths.part.exists() {
                return finalize(
                    request,
                    probe,
                    paths,
                    total,
                    ledger.segments().len(),
                    resumed,
                    started,
                    tracker,
                )
                .await;
            }
            if paths.destination.exists() {
                return Ok(summary_without_transfer(
                    request, probe, paths, total, resumed, started, tracker,
                ));
            }
        }

        if request.resume && !paths.part.exists() && !paths.destination.exists() {
            return Err(NetError::Protocol(
                "resume requested but the .part file is missing".into(),
            ));
        }
        let part = Arc::new(if resumed {
            PartFile::open(&paths.part, total, PreallocateMode::Sparse)?
        } else {
            PartFile::reset(&paths.part, total, PreallocateMode::Sparse)?
        });
        tracker.set_baseline(ledger.downloaded_bytes());
        let mut journal = JournalStore::create(&paths.journal)?;
        let mut state = JournalState {
            task_id: request.id,
            url: request.url.to_string(),
            destination: paths.destination.clone(),
            part_path: paths.part.clone(),
            total_size: total,
            etag: probe.validator.etag.clone(),
            last_modified: probe.validator.last_modified.clone(),
            range_supported: true,
            segments: ledger.snapshot(),
            complete: false,
        };
        journal.append(&state, false)?;

        let limiter = Arc::new(rustget_core::RateLimiter::new(
            self.config.max_speed_bytes_per_sec.unwrap_or(0),
        ));
        let validator = probe.validator.if_range().map(str::to_string);
        let mut worker_urls = vec![request.url.clone()];
        for mirror in &request.mirrors {
            if !worker_urls.contains(mirror) {
                worker_urls.push(mirror.clone());
            }
        }
        let worker_urls = Arc::new(worker_urls);
        let mut join: JoinSet<WorkerReport> = JoinSet::new();
        let mut stops: HashMap<WorkerId, Arc<AtomicU64>> = HashMap::new();
        let mut progress_atoms: HashMap<WorkerId, Arc<AtomicU64>> = HashMap::new();
        let mut next_worker = 10_000_u64;

        for assignment in ledger.initial_assignments() {
            spawn_range_worker(
                &mut join,
                &mut stops,
                &mut progress_atoms,
                &self.client,
                &self.config,
                &part,
                &limiter,
                &worker_urls,
                validator.clone(),
                total,
                self.host_limiter.clone(),
                assignment,
                cancel.clone(),
            );
        }

        let mut ticker = tokio::time::interval(Duration::from_millis(200));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let checkpoint_every = Duration::from_millis(self.config.checkpoint_interval_ms.max(50));
        let mut last_checkpoint = Instant::now();
        let mut last_snapshot = ledger.snapshot();
        let mut speed = SpeedMeter::new();
        let mut adaptive_at = Instant::now();
        let mut adaptive_bytes = ledger.downloaded_bytes();
        let mut adaptive_speed = 0.0_f64;

        loop {
            if ledger.is_complete() {
                break;
            }
            if ledger.active_count() == 0 {
                let mut dispatched = false;
                while let Some(assignment) = ledger.claim(WorkerId(next_worker)) {
                    next_worker += 1;
                    spawn_range_worker(
                        &mut join,
                        &mut stops,
                        &mut progress_atoms,
                        &self.client,
                        &self.config,
                        &part,
                        &limiter,
                        &worker_urls,
                        validator.clone(),
                        total,
                        self.host_limiter.clone(),
                        assignment,
                        cancel.clone(),
                    );
                    dispatched = true;
                    if ledger.active_count() >= usize::from(self.config.max_connections) {
                        break;
                    }
                }
                if !dispatched {
                    return Err(NetError::Protocol(
                        "no active workers and no pending segments".into(),
                    ));
                }
            }

            tokio::select! {
                _ = cancel.cancelled() => {
                    sync_progress(&mut ledger, &progress_atoms);
                    state.segments = ledger.snapshot();
                    let _ = part.sync_data();
                    let _ = journal.append(&state, true);
                    join.abort_all();
                    tracker.emit(
                        DownloadState::Paused,
                        Some(total),
                        ledger.downloaded_bytes(),
                        ledger.active_count(),
                        segment_progress(&ledger),
                        Some(probe.final_url.as_str()),
                        None,
                        None,
                    );
                    return Err(NetError::Cancelled);
                }
                _ = ticker.tick() => {
                    sync_progress(&mut ledger, &progress_atoms);
                    let downloaded = ledger.downloaded_bytes();
                    let elapsed = speed.observe(downloaded);
                    tracker.emit(
                        DownloadState::Downloading,
                        Some(total),
                        downloaded,
                        ledger.active_count(),
                        segment_progress(&ledger),
                        Some(probe.final_url.as_str()),
                        None,
                        None,
                    );
                    let _ = elapsed;
                }
                joined = join.join_next() => {
                    let Some(joined) = joined else {
                        continue;
                    };
                    let report = joined.map_err(|error| {
                        NetError::Protocol(format!("download worker panicked: {error}"))
                    })?;
                    stops.remove(&report.worker);
                    if let Some(atomic) = progress_atoms.remove(&report.worker) {
                        let _ = ledger.report_progress(report.worker, atomic.load(Ordering::Acquire));
                    }
                    match report.outcome {
                        WorkerOutcome::Completed | WorkerOutcome::Stolen => {
                            ledger.finish(report.worker).map_err(|error| {
                                NetError::Protocol(error.to_string())
                            })?;
                            sync_progress(&mut ledger, &progress_atoms);
                            if ledger.is_complete() {
                                break;
                            }
                            dispatch_idle(
                                &mut ledger,
                                report.worker,
                                &mut join,
                                &mut stops,
                                &mut progress_atoms,
                                &self.client,
                                &self.config,
                                &part,
                                &limiter,
                                &worker_urls,
                                validator.clone(),
                                total,
                                self.host_limiter.clone(),
                                cancel.clone(),
                                &mut next_worker,
                            )?;
                        }
                        WorkerOutcome::Cancelled => {
                            join.abort_all();
                            return Err(NetError::Cancelled);
                        }
                        WorkerOutcome::Failed(error) => {
                            let _ = ledger.release(report.worker);
                            sync_progress(&mut ledger, &progress_atoms);
                            state.segments = ledger.snapshot();
                            let _ = part.sync_data();
                            let _ = journal.append(&state, true);
                            join.abort_all();
                            tracker.emit(
                                DownloadState::Failed,
                                Some(total),
                                ledger.downloaded_bytes(),
                                ledger.active_count(),
                                segment_progress(&ledger),
                                Some(probe.final_url.as_str()),
                                None,
                                Some(error.to_string()),
                            );
                            return Err(error);
                        }
                    }
                }
            }

            if last_checkpoint.elapsed() >= checkpoint_every {
                sync_progress(&mut ledger, &progress_atoms);
                let snapshot = ledger.snapshot();
                let changed =
                    serde_json::to_vec(&snapshot).ok() != serde_json::to_vec(&last_snapshot).ok();
                if changed {
                    if self.config.fsync_checkpoints {
                        part.sync_data()?;
                    }
                    state.segments = snapshot.clone();
                    journal.append(&state, self.config.fsync_checkpoints)?;
                    last_snapshot = snapshot;
                }
                last_checkpoint = Instant::now();
            }

            if self.config.adaptive_connections
                && adaptive_at.elapsed() >= Duration::from_millis(self.config.adaptive_interval_ms)
                && !ledger.is_complete()
            {
                let now = Instant::now();
                let elapsed = now.duration_since(adaptive_at).as_secs_f64().max(0.001);
                let current = ledger.downloaded_bytes();
                let measured = current.saturating_sub(adaptive_bytes) as f64 / elapsed;
                let improving = adaptive_speed == 0.0 || measured > adaptive_speed * 1.02;
                if improving
                    && ledger.active_count() < usize::from(self.config.max_connections)
                    && ledger.pending_count() == 0
                {
                    let extra = WorkerId(next_worker);
                    next_worker += 1;
                    if let Ok(Some(split)) = ledger.steal(extra) {
                        if let Some(stop) = stops.get(&split.victim) {
                            stop.store(split.stop_at, Ordering::Release);
                        }
                        spawn_range_worker(
                            &mut join,
                            &mut stops,
                            &mut progress_atoms,
                            &self.client,
                            &self.config,
                            &part,
                            &limiter,
                            &worker_urls,
                            validator.clone(),
                            total,
                            self.host_limiter.clone(),
                            split.assignment,
                            cancel.clone(),
                        );
                        tracing::debug!(
                            connections = ledger.active_count(),
                            measured_bps = measured,
                            "adaptive concurrency grew"
                        );
                    }
                }
                adaptive_at = now;
                adaptive_bytes = current;
                adaptive_speed = measured;
            }
        }

        // All ranges are recorded complete. Drain any no-longer-needed workers.
        join.abort_all();
        while join.join_next().await.is_some() {}

        sync_progress(&mut ledger, &progress_atoms);
        let downloaded = ledger.downloaded_bytes();
        let elapsed = speed.observe(downloaded);
        tracker.emit(
            DownloadState::Finalizing,
            Some(total),
            downloaded,
            ledger.active_count(),
            segment_progress(&ledger),
            Some(probe.final_url.as_str()),
            None,
            None,
        );
        let _ = elapsed;
        state.segments = ledger.snapshot();
        state.complete = true;
        part.sync_all()?;
        journal.append(&state, true)?;
        finalize(
            request,
            probe,
            paths,
            total,
            ledger.segments().len(),
            resumed,
            started,
            tracker,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_single(
        &self,
        request: &DownloadRequest,
        probe: &FileProbe,
        paths: &DownloadPaths,
        cancel: CancellationToken,
        tracker: &mut Tracker,
        started: Instant,
    ) -> Result<DownloadSummary> {
        let total = probe.total_size;
        if paths.destination.exists() && !request.overwrite {
            return Err(NetError::DestinationExists(
                paths.destination.display().to_string(),
            ));
        }
        reset_partial(paths)?;
        tracker.set_baseline(0);
        tracker.emit(
            DownloadState::Downloading,
            total,
            0,
            1,
            Vec::new(),
            Some(probe.final_url.as_str()),
            None,
            None,
        );
        let retry = RetryPolicy::from_engine(&self.config);
        let limiter =
            rustget_core::RateLimiter::new(self.config.max_speed_bytes_per_sec.unwrap_or(0));
        let mut attempt = 0_u32;
        loop {
            match self
                .single_attempt(request, probe, paths, &cancel, tracker, &limiter, total)
                .await
            {
                Ok(downloaded) => {
                    let summary = finalize(
                        request, probe, paths, downloaded, 1, false, started, tracker,
                    )
                    .await?;
                    return Ok(summary);
                }
                Err(NetError::Cancelled) => return Err(NetError::Cancelled),
                Err(error) => {
                    let class = error.class();
                    if !retry.should_retry(class, attempt + 1) {
                        return Err(error);
                    }
                    attempt += 1;
                    let delay = retry.delay(class, attempt);
                    tracing::warn!(
                        attempt,
                        ?class,
                        delay_ms = delay.as_millis(),
                        %error,
                        "single-stream attempt failed; restarting"
                    );
                    tracker.emit(
                        DownloadState::Downloading,
                        total,
                        0,
                        1,
                        Vec::new(),
                        Some(probe.final_url.as_str()),
                        None,
                        Some(error.to_string()),
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn single_attempt(
        &self,
        request: &DownloadRequest,
        probe: &FileProbe,
        paths: &DownloadPaths,
        cancel: &CancellationToken,
        tracker: &mut Tracker,
        limiter: &rustget_core::RateLimiter,
        total: Option<u64>,
    ) -> Result<u64> {
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(NetError::Cancelled),
            response = self.client.apply_common(self.client.client().get(request.url.clone()))?.send() => response?,
        };
        let status = response.status();
        if !status.is_success() {
            return Err(NetError::status(
                status.as_u16(),
                request.url.as_str(),
                status.canonical_reason().unwrap_or("request failed"),
            ));
        }
        let known = total.or_else(|| {
            response
                .headers()
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse().ok())
        });
        if let Some(total) = known {
            if total > 0 && paths.destination.exists() && !request.overwrite {
                return Err(NetError::DestinationExists(
                    paths.destination.display().to_string(),
                ));
            }
        }
        let part = if let Some(total) = known {
            PartFile::open(&paths.part, total, PreallocateMode::Sparse)?
        } else {
            PartFile::open_unbounded(&paths.part)?
        };
        if known == Some(0) {
            part.set_len(0)?;
        }
        let mut stream = response.bytes_stream();
        let mut cursor = 0_u64;
        let mut last_emit = Instant::now();
        let mut meter = SpeedMeter::new();
        loop {
            let next = tokio::select! {
                _ = cancel.cancelled() => return Err(NetError::Cancelled),
                next = tokio::time::timeout(
                    Duration::from_secs(self.config.idle_timeout_secs.max(1)),
                    stream.next(),
                ) => next,
            };
            let chunk = match next {
                Err(_) => {
                    return Err(NetError::Protocol(format!(
                        "no data received for {}s",
                        self.config.idle_timeout_secs
                    )));
                }
                Ok(None) => break,
                Ok(Some(Err(error))) => return Err(NetError::Request(error)),
                Ok(Some(Ok(chunk))) => chunk,
            };
            if let Some(total) = known {
                let room = total.saturating_sub(cursor);
                let take = usize::try_from(room.min(chunk.len() as u64)).unwrap_or(0);
                if take > 0 {
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(NetError::Cancelled),
                        _ = limiter.acquire(take as u64) => {}
                    }
                    part.write_at(cursor, &chunk[..take])?;
                    cursor += take as u64;
                }
                if cursor >= total {
                    break;
                }
            } else {
                let bytes = chunk.len() as u64;
                tokio::select! {
                    _ = cancel.cancelled() => return Err(NetError::Cancelled),
                    _ = limiter.acquire(bytes) => {}
                }
                part.write_at(cursor, &chunk)?;
                cursor += bytes;
            }
            if last_emit.elapsed() >= Duration::from_millis(200) {
                meter.observe(cursor);
                tracker.emit(
                    DownloadState::Downloading,
                    known,
                    cursor,
                    1,
                    vec![SegmentProgress {
                        id: SegmentId(0),
                        start: 0,
                        end: known.unwrap_or(cursor).saturating_sub(1),
                        written: cursor,
                        state: SegmentState::Active,
                        attempts: 1,
                    }],
                    Some(probe.final_url.as_str()),
                    None,
                    None,
                );
                last_emit = Instant::now();
            }
        }
        if let Some(total) = known {
            if cursor != total {
                return Err(NetError::ShortRead(total.saturating_sub(cursor)));
            }
            part.set_len(total)?;
        } else {
            part.set_len(cursor)?;
        }
        part.sync_all()?;
        Ok(cursor)
    }
}

#[derive(Debug)]
enum WorkerOutcome {
    Completed,
    Stolen,
    Cancelled,
    Failed(NetError),
}

#[derive(Debug)]
struct WorkerReport {
    worker: WorkerId,
    outcome: WorkerOutcome,
}

#[allow(clippy::too_many_arguments)]
fn spawn_range_worker(
    join: &mut JoinSet<WorkerReport>,
    stops: &mut HashMap<WorkerId, Arc<AtomicU64>>,
    progress_atoms: &mut HashMap<WorkerId, Arc<AtomicU64>>,
    client: &Arc<NetClient>,
    config: &EngineConfig,
    part: &Arc<PartFile>,
    limiter: &Arc<rustget_core::RateLimiter>,
    urls: &Arc<Vec<Url>>,
    validator: Option<String>,
    total: u64,
    host_limiter: Option<Arc<crate::host::HostLimiter>>,
    assignment: Assignment,
    cancel: CancellationToken,
) {
    let stop = Arc::new(AtomicU64::new(u64::MAX));
    let progress = Arc::new(AtomicU64::new(assignment.range.start));
    stops.insert(assignment.worker, Arc::clone(&stop));
    progress_atoms.insert(assignment.worker, Arc::clone(&progress));
    let client = Arc::clone(client);
    let part = Arc::clone(part);
    let limiter = Arc::clone(limiter);
    let urls = Arc::clone(urls);
    let config = config.clone();
    join.spawn(async move {
        let worker = assignment.worker;
        let outcome = range_worker(
            &assignment,
            client,
            config,
            part,
            limiter,
            urls,
            validator,
            total,
            host_limiter,
            stop,
            progress,
            cancel,
        )
        .await;
        WorkerReport { worker, outcome }
    });
}

#[allow(clippy::too_many_arguments)]
async fn range_worker(
    assignment: &Assignment,
    client: Arc<NetClient>,
    config: EngineConfig,
    part: Arc<PartFile>,
    limiter: Arc<rustget_core::RateLimiter>,
    urls: Arc<Vec<Url>>,
    validator: Option<String>,
    total: u64,
    host_limiter: Option<Arc<crate::host::HostLimiter>>,
    stop: Arc<AtomicU64>,
    progress: Arc<AtomicU64>,
    cancel: CancellationToken,
) -> WorkerOutcome {
    let retry = RetryPolicy::from_engine(&config);
    let mut cursor = assignment.range.start;
    let mut attempt = 0_u32;
    let mut url_index = 0_usize;
    loop {
        let Some(url) = urls.get(url_index) else {
            return WorkerOutcome::Failed(NetError::Protocol("mirror list is empty".into()));
        };
        let permit = if let Some(limiter) = &host_limiter {
            Some(limiter.acquire(url).await)
        } else {
            None
        };
        let transfer = transfer_range(
            &client,
            &config,
            &part,
            &limiter,
            url,
            validator.as_deref(),
            total,
            assignment.range.end,
            &stop,
            &progress,
            &cancel,
            &mut cursor,
        )
        .await;
        drop(permit);
        match transfer {
            Ok(TransferEnd::Completed) => return WorkerOutcome::Completed,
            Ok(TransferEnd::Stolen) => return WorkerOutcome::Stolen,
            Err(NetError::Cancelled) => return WorkerOutcome::Cancelled,
            Err(error) => {
                let class = error.class();
                if !retry.should_retry(class, attempt + 1) {
                    return WorkerOutcome::Failed(error);
                }
                attempt += 1;
                if urls.len() > 1
                    && matches!(
                        class,
                        rustget_core::ErrorClass::Transient | rustget_core::ErrorClass::RateLimited
                    )
                {
                    url_index = (url_index + 1) % urls.len();
                }
                let delay = retry.delay(class, attempt);
                tracing::debug!(
                    worker = assignment.worker.0,
                    segment = assignment.segment.0,
                    attempt,
                    ?class,
                    delay_ms = delay.as_millis(),
                    %error,
                    "range attempt failed"
                );
                tokio::select! {
                    _ = cancel.cancelled() => return WorkerOutcome::Cancelled,
                    _ = tokio::time::sleep(delay) => {}
                }
            }
        }
    }
}

enum TransferEnd {
    Completed,
    Stolen,
}

#[allow(clippy::too_many_arguments)]
async fn transfer_range(
    client: &NetClient,
    config: &EngineConfig,
    part: &PartFile,
    limiter: &rustget_core::RateLimiter,
    url: &Url,
    validator: Option<&str>,
    expected_total: u64,
    end: u64,
    stop: &AtomicU64,
    progress: &AtomicU64,
    cancel: &CancellationToken,
    cursor: &mut u64,
) -> Result<TransferEnd> {
    let mut request = client
        .apply_common(client.client().get(url.clone()))?
        .header(reqwest::header::RANGE, format!("bytes={}-{end}", *cursor));
    if let Some(validator) = validator {
        request = request.header(reqwest::header::IF_RANGE, validator);
    }
    let response = tokio::select! {
        _ = cancel.cancelled() => return Err(NetError::Cancelled),
        response = request.send() => response?,
    };
    let status = response.status();
    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        let header = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| NetError::Header("206 without Content-Range".into()))?;
        let (start, _end, response_total) = parse_content_range(header)
            .ok_or_else(|| NetError::Header(format!("invalid Content-Range '{header}'")))?;
        if start != *cursor {
            return Err(NetError::ContentRangeMismatch(header.to_string(), *cursor));
        }
        if response_total.is_some_and(|total| total != expected_total) {
            return Err(NetError::RemoteChanged(format!(
                "Content-Range total {response_total:?} differs from probed size {expected_total}"
            )));
        }
    } else if status == reqwest::StatusCode::OK {
        if validator.is_some() {
            return Err(NetError::RemoteChanged(
                "server ignored If-Range because the resource changed".into(),
            ));
        }
        return Err(NetError::RangeUnsupported);
    } else if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        return Err(NetError::RemoteChanged(
            "server rejected the requested range (416)".into(),
        ));
    } else {
        return Err(NetError::status(
            status.as_u16(),
            url.as_str(),
            status.canonical_reason().unwrap_or("range request failed"),
        ));
    }

    let mut stream = response.bytes_stream();
    loop {
        if let Some(boundary) = stop_boundary(stop, *cursor) {
            if *cursor >= boundary {
                return Ok(TransferEnd::Stolen);
            }
        }
        let next = tokio::select! {
            _ = cancel.cancelled() => return Err(NetError::Cancelled),
            next = tokio::time::timeout(
                Duration::from_secs(config.idle_timeout_secs.max(1)),
                stream.next(),
            ) => next,
        };
        let chunk = match next {
            Err(_) => {
                return Err(NetError::Protocol(format!(
                    "no data received for {}s",
                    config.idle_timeout_secs
                )));
            }
            Ok(None) => {
                if *cursor > end {
                    return Ok(TransferEnd::Completed);
                }
                return Err(NetError::ShortRead(end + 1 - *cursor));
            }
            Ok(Some(Err(error))) => return Err(NetError::Request(error)),
            Ok(Some(Ok(chunk))) => chunk,
        };
        let boundary = stop_boundary(stop, *cursor).unwrap_or(end + 1);
        let room = (end + 1).saturating_sub(*cursor);
        let allowed = chunk.len() as u64;
        let allowed = allowed.min(room).min(boundary.saturating_sub(*cursor));
        if allowed == 0 {
            if *cursor >= boundary {
                return Ok(TransferEnd::Stolen);
            }
            return Ok(TransferEnd::Completed);
        }
        tokio::select! {
            _ = cancel.cancelled() => return Err(NetError::Cancelled),
            _ = limiter.acquire(allowed) => {}
        }
        // A split may have arrived while waiting for a rate-limit token.
        let boundary_now = stop_boundary(stop, *cursor);
        let allowed = boundary_now.map_or(allowed, |boundary| {
            allowed.min(boundary.saturating_sub(*cursor))
        });
        if allowed == 0 {
            return Ok(TransferEnd::Stolen);
        }
        part.write_at(*cursor, &chunk[..usize::try_from(allowed).unwrap_or(0)])?;
        *cursor += allowed;
        progress.store(*cursor, Ordering::Release);
        if boundary_now.is_some_and(|boundary| *cursor >= boundary) {
            return Ok(TransferEnd::Stolen);
        }
        if *cursor > end {
            return Ok(TransferEnd::Completed);
        }
    }
}

fn stop_boundary(stop: &AtomicU64, cursor: u64) -> Option<u64> {
    let value = stop.load(Ordering::Acquire);
    if value == u64::MAX {
        None
    } else {
        Some(value.max(cursor))
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatch_idle(
    ledger: &mut SegmentLedger,
    idle: WorkerId,
    join: &mut JoinSet<WorkerReport>,
    stops: &mut HashMap<WorkerId, Arc<AtomicU64>>,
    progress_atoms: &mut HashMap<WorkerId, Arc<AtomicU64>>,
    client: &Arc<NetClient>,
    config: &EngineConfig,
    part: &Arc<PartFile>,
    limiter: &Arc<rustget_core::RateLimiter>,
    urls: &Arc<Vec<Url>>,
    validator: Option<String>,
    total: u64,
    host_limiter: Option<Arc<crate::host::HostLimiter>>,
    cancel: CancellationToken,
    next_worker: &mut u64,
) -> Result<()> {
    if let Some(split) = ledger
        .steal(idle)
        .map_err(|error| NetError::Protocol(error.to_string()))?
    {
        if let Some(stop) = stops.get(&split.victim) {
            stop.store(split.stop_at, Ordering::Release);
        }
        spawn_range_worker(
            join,
            stops,
            progress_atoms,
            client,
            config,
            part,
            limiter,
            urls,
            validator,
            total,
            host_limiter.clone(),
            split.assignment,
            cancel,
        );
        return Ok(());
    }
    if let Some(assignment) = ledger.claim(idle) {
        spawn_range_worker(
            join,
            stops,
            progress_atoms,
            client,
            config,
            part,
            limiter,
            urls,
            validator,
            total,
            host_limiter.clone(),
            assignment,
            cancel,
        );
        return Ok(());
    }
    // No splittable work now. The idle worker is intentionally not respawned;
    // another completion will trigger another dispatch.
    let _ = next_worker;
    Ok(())
}

fn sync_progress(ledger: &mut SegmentLedger, atoms: &HashMap<WorkerId, Arc<AtomicU64>>) {
    for (worker, atomic) in atoms {
        let _ = ledger.report_progress(*worker, atomic.load(Ordering::Acquire));
    }
}

fn segment_progress(ledger: &SegmentLedger) -> Vec<SegmentProgress> {
    ledger
        .segments()
        .iter()
        .map(|segment| SegmentProgress {
            id: segment.id,
            start: segment.range.start,
            end: segment.range.end,
            written: segment.written,
            state: segment.state,
            attempts: segment.attempts,
        })
        .collect()
}

fn ftp_error(error: suppaftp::FtpError) -> NetError {
    NetError::Ftp(error.to_string())
}

fn ftp_destination(request: &DownloadRequest) -> Result<PathBuf> {
    let explicit = request.destination.is_some();
    let name = rustget_storage::filename_from_url(&request.url)
        .map(|name| sanitize_filename(&name))
        .unwrap_or_else(|| "download".into());
    let destination = if let Some(path) = &request.destination {
        if path.is_dir() {
            path.join(name)
        } else {
            path.clone()
        }
    } else {
        request.directory.join(name)
    };
    if destination.exists() && !request.overwrite {
        if explicit {
            return Err(NetError::DestinationExists(
                destination.display().to_string(),
            ));
        }
        return Ok(unique_destination(destination));
    }
    Ok(destination)
}

fn is_hls_url(url: &Url) -> bool {
    url.path().to_ascii_lowercase().ends_with(".m3u8")
}

fn is_mpd_url(url: &Url) -> bool {
    url.path().to_ascii_lowercase().ends_with(".mpd")
}

fn dash_destination(request: &DownloadRequest) -> Result<PathBuf> {
    let explicit = request.destination.is_some();
    let downloaded_name = rustget_storage::filename_from_url(&request.url)
        .map(|name| sanitize_filename(&name))
        .unwrap_or_else(|| "stream.mpd".into());
    let stem = downloaded_name
        .strip_suffix(".mpd")
        .or_else(|| downloaded_name.strip_suffix(".MPD"))
        .unwrap_or(&downloaded_name)
        .to_string();
    let destination = if let Some(path) = &request.destination {
        if path.is_dir() {
            path.join(format!("{stem}.mp4"))
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("mpd"))
        {
            path.with_extension("mp4")
        } else {
            path.clone()
        }
    } else {
        request.directory.join(format!("{stem}.mp4"))
    };
    if destination.exists() && !request.overwrite {
        if explicit {
            return Err(NetError::DestinationExists(
                destination.display().to_string(),
            ));
        }
        return Ok(unique_destination(destination));
    }
    Ok(destination)
}

fn hls_destination(request: &DownloadRequest, playlist_url: &Url) -> Result<PathBuf> {
    let explicit = request.destination.is_some();
    let downloaded_name = rustget_storage::filename_from_url(playlist_url)
        .map(|name| sanitize_filename(&name))
        .unwrap_or_else(|| "stream.m3u8".into());
    let stem = downloaded_name
        .strip_suffix(".m3u8")
        .or_else(|| downloaded_name.strip_suffix(".M3U8"))
        .unwrap_or(&downloaded_name)
        .to_string();
    let destination = if let Some(path) = &request.destination {
        if path.is_dir() {
            path.join(format!("{stem}.ts"))
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("m3u8"))
        {
            path.with_extension("ts")
        } else {
            path.clone()
        }
    } else {
        request.directory.join(format!("{stem}.ts"))
    };
    if destination.exists() && !request.overwrite {
        if explicit {
            return Err(NetError::DestinationExists(
                destination.display().to_string(),
            ));
        }
        return Ok(unique_destination(destination));
    }
    Ok(destination)
}

fn resolve_destination(request: &DownloadRequest, probe: &FileProbe) -> Result<PathBuf> {
    let explicit = request.destination.is_some();
    let destination = if let Some(destination) = &request.destination {
        if destination.is_dir() {
            destination.join(sanitize_filename(&probe.filename))
        } else {
            destination.clone()
        }
    } else {
        request.directory.join(sanitize_filename(&probe.filename))
    };
    if !request.resume && destination.exists() && !request.overwrite {
        if explicit {
            return Err(NetError::DestinationExists(
                destination.display().to_string(),
            ));
        }
        return Ok(unique_destination(destination));
    }
    if request.overwrite && destination.exists() {
        std::fs::remove_file(&destination)?;
    }
    Ok(destination)
}

fn reset_partial(paths: &DownloadPaths) -> Result<()> {
    for path in [&paths.part, &paths.journal] {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn finish_empty(
    request: &DownloadRequest,
    probe: &FileProbe,
    paths: &DownloadPaths,
    started: Instant,
) -> Result<DownloadSummary> {
    let part = PartFile::reset(&paths.part, 0, PreallocateMode::None)?;
    part.sync_all()?;
    std::fs::rename(&paths.part, &paths.destination)?;
    remove_journal(paths);
    Ok(DownloadSummary {
        id: request.id,
        destination: paths.destination.clone(),
        final_url: probe.final_url.to_string(),
        total_size: 0,
        downloaded: 0,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        segments: 0,
        resumed: false,
        checksum_verified: false,
        average_speed_bps: 0.0,
    })
}

#[allow(clippy::too_many_arguments)]
async fn finalize(
    request: &DownloadRequest,
    probe: &FileProbe,
    paths: &DownloadPaths,
    total: u64,
    segments: usize,
    resumed: bool,
    started: Instant,
    tracker: &mut Tracker,
) -> Result<DownloadSummary> {
    if let Some(spec) = &request.checksum {
        verify_checksum(&paths.part, spec).await?;
    }
    if paths.destination.exists() {
        if request.overwrite {
            std::fs::remove_file(&paths.destination)?;
        } else {
            return Err(NetError::DestinationExists(
                paths.destination.display().to_string(),
            ));
        }
    }
    std::fs::rename(&paths.part, &paths.destination)?;
    remove_journal(paths);
    let elapsed = started.elapsed();
    let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    let summary = DownloadSummary {
        id: request.id,
        destination: paths.destination.clone(),
        final_url: probe.final_url.to_string(),
        total_size: total,
        downloaded: total,
        elapsed_ms,
        segments,
        resumed,
        checksum_verified: request.checksum.is_some(),
        average_speed_bps: if elapsed.as_secs_f64() > 0.0 {
            total as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        },
    };
    tracker.emit(
        DownloadState::Complete,
        Some(total),
        total,
        0,
        Vec::new(),
        Some(probe.final_url.as_str()),
        None,
        None,
    );
    Ok(summary)
}

#[allow(clippy::too_many_arguments)]
fn summary_without_transfer(
    request: &DownloadRequest,
    probe: &FileProbe,
    paths: &DownloadPaths,
    total: u64,
    resumed: bool,
    started: Instant,
    tracker: &mut Tracker,
) -> DownloadSummary {
    tracker.emit(
        DownloadState::Complete,
        Some(total),
        total,
        0,
        Vec::new(),
        Some(probe.final_url.as_str()),
        None,
        None,
    );
    DownloadSummary {
        id: request.id,
        destination: paths.destination.clone(),
        final_url: probe.final_url.to_string(),
        total_size: total,
        downloaded: total,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        segments: 0,
        resumed,
        checksum_verified: false,
        average_speed_bps: 0.0,
    }
}

fn remove_journal(paths: &DownloadPaths) {
    let _ = std::fs::remove_file(&paths.journal);
    let mut tmp = paths.journal.as_os_str().to_os_string();
    tmp.push(".tmp");
    let _ = std::fs::remove_file(Path::new(&tmp));
}

struct Tracker {
    tx: watch::Sender<DownloadProgress>,
    id: Uuid,
    last_bytes: u64,
    last_sample: Instant,
    speed: f64,
    units: Option<(u64, u64)>,
}

impl Tracker {
    fn new(tx: watch::Sender<DownloadProgress>, id: Uuid) -> Self {
        Self {
            tx,
            id,
            last_bytes: 0,
            last_sample: Instant::now(),
            speed: 0.0,
            units: None,
        }
    }

    fn set_units(&mut self, total: u64, completed: u64) {
        self.units = Some((total, completed));
    }

    /// Establish the resumed byte count so the first progress event does not
    /// report bytes downloaded by a previous process as instantaneous speed.
    fn set_baseline(&mut self, downloaded: u64) {
        self.last_bytes = downloaded;
        self.last_sample = Instant::now();
        self.speed = 0.0;
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        state: DownloadState,
        total: Option<u64>,
        downloaded: u64,
        connections: usize,
        segments: Vec<SegmentProgress>,
        final_url: Option<&str>,
        filename: Option<String>,
        error: Option<String>,
    ) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_sample).as_secs_f64();
        if elapsed >= 0.05 && downloaded >= self.last_bytes {
            let instant = (downloaded - self.last_bytes) as f64 / elapsed;
            if downloaded > self.last_bytes {
                self.speed = if self.speed == 0.0 {
                    instant
                } else {
                    self.speed * 0.7 + instant * 0.3
                };
            } else {
                self.speed *= 0.5;
            }
            self.last_bytes = downloaded;
            self.last_sample = now;
        }
        if matches!(state, DownloadState::Complete) {
            self.speed = 0.0;
        }
        let speed = self.speed;
        let eta = total.and_then(|total| {
            (speed > 1.0 && total > downloaded)
                .then_some(((total - downloaded) as f64 / speed) as u64)
        });
        let _ = self.tx.send(DownloadProgress {
            task_id: self.id,
            state,
            total_size: total,
            downloaded,
            speed_bps: speed,
            eta_secs: eta,
            active_connections: connections,
            segments,
            final_url: final_url.map(str::to_string),
            filename,
            error,
            unit_total: self.units.map(|units| units.0),
            unit_completed: self.units.map_or(0, |units| units.1),
        });
    }
}

struct SpeedMeter {
    last: Instant,
    last_bytes: u64,
    smoothed: f64,
}

impl SpeedMeter {
    fn new() -> Self {
        Self {
            last: Instant::now(),
            last_bytes: 0,
            smoothed: 0.0,
        }
    }

    fn observe(&mut self, bytes: u64) -> f64 {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        if elapsed > 0.0 {
            let instant = (bytes.saturating_sub(self.last_bytes)) as f64 / elapsed;
            self.smoothed = if self.smoothed == 0.0 {
                instant
            } else {
                self.smoothed * 0.7 + instant * 0.3
            };
            self.last = now;
            self.last_bytes = bytes;
        }
        self.smoothed
    }
}

/// Merge any adjacent ranges (used by tests and journal diagnostics).
#[must_use]
pub fn merge_ranges(mut ranges: Vec<ByteRange>) -> Vec<ByteRange> {
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<ByteRange> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end.saturating_add(1) => {
                last.end = last.end.max(range.end);
            }
            _ => merged.push(range),
        }
    }
    merged
}
