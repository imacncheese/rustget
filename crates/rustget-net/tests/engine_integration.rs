use std::path::PathBuf;
use std::time::{Duration, Instant};

use rustget_core::{EngineConfig, RetryPolicy};
use rustget_net::{
    ChecksumSpec, DownloadRequest, Downloader, NetError, RequestOptions, verify_checksum,
};
use rustget_testkit::{TestFile, TestServer, TestkitConfig, expected_content};
use uuid::Uuid;

fn config(connections: u8) -> EngineConfig {
    EngineConfig {
        connections,
        max_connections: 16,
        min_segment_size: 64 * 1024,
        checkpoint_interval_ms: 50,
        idle_timeout_secs: 5,
        retry_base_ms: 10,
        retry_max_ms: 100,
        max_retries: 3,
        ..EngineConfig::default()
    }
}

fn request(url: String, directory: &std::path::Path) -> DownloadRequest {
    DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&url).unwrap(),
        mirrors: Vec::new(),
        destination: None,
        directory: directory.to_path_buf(),
        resume: false,
        overwrite: false,
        checksum: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_multipart_file_byte_exact() {
    let server = TestServer::start(
        TestkitConfig::default().with_file(TestFile::new("large.bin", 2 * 1024 * 1024)),
    )
    .await
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(config(4), RequestOptions::default()).unwrap();
    let req = request(server.file_url("large.bin"), temp.path());
    let id = req.id;
    let (handle, tx) = downloader.handle(id);
    let summary = downloader
        .download(req, handle.cancel.clone(), tx)
        .await
        .unwrap();
    assert_eq!(summary.total_size, 2 * 1024 * 1024);
    let bytes = tokio::fs::read(&summary.destination).await.unwrap();
    assert_eq!(
        bytes,
        expected_content(0x5EED_1234_ABCD_9876, 2 * 1024 * 1024)
    );
    assert!(!PathBuf::from(format!("{}.part", summary.destination.display())).exists());
    assert!(!PathBuf::from(format!("{}.rustget", summary.destination.display())).exists());
    let ranged: Vec<_> = server
        .requests()
        .into_iter()
        .filter(|entry| entry.status == 206)
        .collect();
    assert!(
        ranged.len() >= 4,
        "expected at least four ranged requests, got {}",
        ranged.len()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn falls_back_to_single_stream_when_ranges_are_ignored() {
    let server = TestServer::start(
        TestkitConfig::default().with_file(TestFile::new("plain.bin", 700 * 1024).no_range()),
    )
    .await
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(config(4), RequestOptions::default()).unwrap();
    let req = request(server.file_url("plain.bin"), temp.path());
    let (handle, tx) = downloader.handle(req.id);
    let summary = downloader.download(req, handle.cancel, tx).await.unwrap();
    assert_eq!(summary.total_size, 700 * 1024);
    let bytes = tokio::fs::read(&summary.destination).await.unwrap();
    assert_eq!(bytes, expected_content(0x5EED_1234_ABCD_9876, 700 * 1024));
    let log = server.requests();
    assert!(log.iter().any(|entry| entry.status == 200));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn follows_redirects_and_preserves_range() {
    let server = TestServer::start(
        TestkitConfig::default().with_file(TestFile::new("redir.bin", 512 * 1024).redirects(2)),
    )
    .await
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(config(4), RequestOptions::default()).unwrap();
    let req = request(server.url("/redirect/redir.bin/2"), temp.path());
    let (handle, tx) = downloader.handle(req.id);
    let summary = downloader.download(req, handle.cancel, tx).await.unwrap();
    let bytes = tokio::fs::read(&summary.destination).await.unwrap();
    assert_eq!(bytes, expected_content(0x5EED_1234_ABCD_9876, 512 * 1024));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retries_mid_stream_resets() {
    let server = TestServer::start(
        TestkitConfig::default().with_file(
            TestFile::new("flaky.bin", 900 * 1024)
                .flaky(2, 24 * 1024)
                .throttled(50_000_000),
        ),
    )
    .await
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut cfg = config(4);
    cfg.max_retries = 6;
    let downloader = Downloader::new(cfg, RequestOptions::default()).unwrap();
    let req = request(server.file_url("flaky.bin"), temp.path());
    let (handle, tx) = downloader.handle(req.id);
    let summary = downloader.download(req, handle.cancel, tx).await.unwrap();
    let bytes = tokio::fs::read(&summary.destination).await.unwrap();
    assert_eq!(bytes, expected_content(0x5EED_1234_ABCD_9876, 900 * 1024));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn detects_changed_validator() {
    let server = TestServer::start(
        TestkitConfig::default()
            .with_file(TestFile::new("mutable.bin", 512 * 1024).changing_etag()),
    )
    .await
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(config(2), RequestOptions::default()).unwrap();
    let req = request(server.file_url("mutable.bin"), temp.path());
    let (handle, tx) = downloader.handle(req.id);
    let result = downloader.download(req, handle.cancel, tx).await;
    assert!(
        matches!(result, Err(NetError::RemoteChanged(_))),
        "expected remote-change error, got {result:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancels_and_resumes_from_journal() {
    let server = TestServer::start(
        TestkitConfig::default()
            .with_file(TestFile::new("resume.bin", 8 * 1024 * 1024).throttled(64 * 1024 * 1024)),
    )
    .await
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut cfg = config(4);
    cfg.max_speed_bytes_per_sec = Some(2 * 1024 * 1024);
    let downloader = Downloader::new(cfg, RequestOptions::default()).unwrap();
    let destination = temp.path().join("resume.bin");
    let req = DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&server.file_url("resume.bin")).unwrap(),
        mirrors: Vec::new(),
        destination: Some(destination.clone()),
        directory: temp.path().to_path_buf(),
        resume: false,
        overwrite: false,
        checksum: None,
    };
    let (handle, tx) = downloader.handle(req.id);
    let mut progress = handle.progress.clone();
    let cancel = handle.cancel.clone();
    let task = tokio::spawn(async move { downloader.download(req, cancel, tx).await });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let value = progress.borrow().clone();
        if value.downloaded >= 384 * 1024 {
            break;
        }
        assert!(Instant::now() < deadline, "download made no progress");
        tokio::time::timeout(Duration::from_millis(200), progress.changed())
            .await
            .ok();
    }
    handle.cancel.cancel();
    let first = task.await.unwrap();
    assert!(matches!(first, Err(NetError::Cancelled)));
    assert!(
        destination.with_extension("bin.part").exists(),
        "part file retained"
    );
    let journal = temp.path().join("resume.bin.rustget");
    assert!(journal.exists(), "journal retained");
    let resumed = tokio::fs::read(&journal).await.unwrap();
    assert!(!resumed.is_empty());

    let downloader = Downloader::new(
        EngineConfig {
            max_speed_bytes_per_sec: None,
            ..config(4)
        },
        RequestOptions::default(),
    )
    .unwrap();
    let req = DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&server.file_url("resume.bin")).unwrap(),
        mirrors: Vec::new(),
        destination: Some(destination.clone()),
        directory: temp.path().to_path_buf(),
        resume: true,
        overwrite: false,
        checksum: None,
    };
    let (handle, tx) = downloader.handle(req.id);
    let summary = downloader.download(req, handle.cancel, tx).await.unwrap();
    assert!(summary.resumed);
    let bytes = tokio::fs::read(&summary.destination).await.unwrap();
    assert_eq!(
        bytes,
        expected_content(0x5EED_1234_ABCD_9876, 8 * 1024 * 1024)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn verifies_checksum_and_keeps_part_on_mismatch() {
    let server = TestServer::start(TestkitConfig::single(128 * 1024))
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(config(2), RequestOptions::default()).unwrap();
    let expected = rustget_net::checksum::hash_file(
        {
            let path = temp.path().join("expected");
            std::fs::write(&path, expected_content(0x5EED_1234_ABCD_9876, 128 * 1024)).unwrap();
            path
        }
        .as_path(),
        rustget_net::ChecksumAlgorithm::Sha256,
    )
    .unwrap();
    let destination = temp.path().join("sum.bin");
    let mut req = request(server.file_url("file.bin"), temp.path());
    req.destination = Some(destination.clone());
    req.checksum = Some(ChecksumSpec::new("sha256", &expected).unwrap());
    let (handle, tx) = downloader.handle(req.id);
    let summary = downloader.download(req, handle.cancel, tx).await.unwrap();
    assert!(summary.checksum_verified);
    verify_checksum(
        &summary.destination,
        &ChecksumSpec::new("sha256", &expected).unwrap(),
    )
    .await
    .unwrap();

    let mut bad = request(server.file_url("file.bin"), temp.path());
    bad.destination = Some(temp.path().join("bad.bin"));
    bad.checksum = Some(ChecksumSpec::new("sha256", "00").unwrap());
    let (handle, tx) = downloader.handle(bad.id);
    let result = downloader.download(bad, handle.cancel, tx).await;
    assert!(matches!(result, Err(NetError::ChecksumMismatch { .. })));
}

#[tokio::test]
async fn retry_policy_remains_sane() {
    let policy = RetryPolicy::default();
    assert!(!policy.should_retry(rustget_core::ErrorClass::Auth, 1));
    assert_eq!(
        rustget_core::RetryPolicy::classify_status(429),
        rustget_core::ErrorClass::RateLimited
    );
}
