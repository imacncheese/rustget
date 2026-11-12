use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_net::{DownloadRequest, Downloader, RequestOptions};
use rustget_testkit::{TestFile, TestServer, TestkitConfig, expected_content};
use tokio::time::timeout;
use uuid::Uuid;

fn request(url: String, mirrors: Vec<String>, temp: &std::path::Path) -> DownloadRequest {
    DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&url).unwrap(),
        mirrors: mirrors
            .into_iter()
            .map(|mirror| url::Url::parse(&mirror).unwrap())
            .collect(),
        destination: None,
        directory: temp.to_path_buf(),
        resume: false,
        overwrite: false,
        checksum: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fails_over_to_mirror_after_primary_resets() {
    let size = 2 * 1024 * 1024;
    let primary = TestServer::start(
        TestkitConfig::default().with_file(
            TestFile::new("mirror.bin", size)
                .flaky(100, 128 * 1024)
                .throttled(64 * 1024 * 1024),
        ),
    )
    .await
    .unwrap();
    let mirror =
        TestServer::start(TestkitConfig::default().with_file(TestFile::new("mirror.bin", size)))
            .await
            .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let cfg = EngineConfig {
        connections: 1,
        max_connections: 1,
        min_segment_size: size,
        max_retries: 8,
        retry_base_ms: 10,
        adaptive_connections: false,
        ..EngineConfig::default()
    };
    let downloader = Downloader::new(cfg, RequestOptions::default()).unwrap();
    let req = request(
        primary.file_url("mirror.bin"),
        vec![mirror.file_url("mirror.bin")],
        temp.path(),
    );
    let (handle, tx) = downloader.handle(req.id);
    let summary = timeout(
        Duration::from_secs(20),
        downloader.download(req, handle.cancel, tx),
    )
    .await
    .expect("mirror failover timed out")
    .unwrap();
    let bytes = tokio::fs::read(&summary.destination).await.unwrap();
    assert_eq!(bytes, expected_content(0x5EED_1234_ABCD_9876, size));
    assert!(
        mirror
            .requests()
            .iter()
            .any(|entry| entry.range_start.unwrap_or(0) > 0),
        "mirror never served a resumed range"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn probes_mirror_when_primary_is_unreachable() {
    let mirror = TestServer::start(TestkitConfig::single(256 * 1024))
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(EngineConfig::default(), RequestOptions::default()).unwrap();
    let req = request(
        "http://127.0.0.1:1/unreachable".into(),
        vec![mirror.file_url("file.bin")],
        temp.path(),
    );
    let (handle, tx) = downloader.handle(req.id);
    let summary = timeout(
        Duration::from_secs(10),
        downloader.download(req, handle.cancel, tx),
    )
    .await
    .expect("probe failover timed out")
    .unwrap();
    let bytes = tokio::fs::read(&summary.destination).await.unwrap();
    assert_eq!(bytes, expected_content(0x5EED_1234_ABCD_9876, 256 * 1024));
}
