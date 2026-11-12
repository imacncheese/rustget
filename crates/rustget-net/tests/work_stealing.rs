use std::collections::BTreeSet;
use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_net::{DownloadRequest, Downloader, RequestOptions};
use rustget_testkit::{TestFile, TestServer, TestkitConfig, expected_content};
use tokio::time::timeout;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn steals_work_from_a_slow_segment_and_still_writes_exact_bytes() {
    let size = 8 * 1024 * 1024;
    let server = TestServer::start(
        TestkitConfig::default().with_file(
            TestFile::new("slow.bin", size)
                .slow_first_range(700)
                .throttled(64 * 1024 * 1024),
        ),
    )
    .await
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let cfg = EngineConfig {
        connections: 4,
        max_connections: 12,
        min_segment_size: 128 * 1024,
        checkpoint_interval_ms: 50,
        ..EngineConfig::default()
    };
    let downloader = Downloader::new(cfg, RequestOptions::default()).unwrap();
    let req = DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&server.file_url("slow.bin")).unwrap(),
        mirrors: Vec::new(),
        destination: None,
        directory: temp.path().to_path_buf(),
        resume: false,
        overwrite: false,
        checksum: None,
    };
    let (handle, tx) = downloader.handle(req.id);
    let summary = timeout(
        Duration::from_secs(20),
        downloader.download(req, handle.cancel, tx),
    )
    .await
    .expect("transfer timed out")
    .unwrap();
    let bytes = tokio::fs::read(&summary.destination).await.unwrap();
    assert_eq!(bytes, expected_content(0x5EED_1234_ABCD_9876, size));
    let starts: BTreeSet<u64> = server
        .requests()
        .into_iter()
        .filter(|entry| entry.status == 206)
        .filter_map(|entry| entry.range_start)
        .collect();
    assert!(
        starts.len() > 4,
        "work stealing should create more than the initial four ranges: {starts:?}"
    );
}
