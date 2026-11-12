use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_net::{DownloadRequest, Downloader, RequestOptions};
use rustget_testkit::{TestFile, TestServer, TestkitConfig, expected_content};
use tokio::time::timeout;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn grows_connections_when_throughput_improves() {
    let size = 8 * 1024 * 1024;
    let server = TestServer::start(
        TestkitConfig::default()
            .with_file(TestFile::new("adaptive.bin", size).throttled(4 * 1024 * 1024)),
    )
    .await
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let cfg = EngineConfig {
        connections: 2,
        max_connections: 8,
        min_segment_size: 128 * 1024,
        adaptive_connections: true,
        adaptive_interval_ms: 200,
        checkpoint_interval_ms: 100,
        ..EngineConfig::default()
    };
    let downloader = Downloader::new(cfg, RequestOptions::default()).unwrap();
    let req = DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&server.file_url("adaptive.bin")).unwrap(),
        mirrors: Vec::new(),
        media_variant: None,
        destination: None,
        directory: temp.path().to_path_buf(),
        resume: false,
        overwrite: false,
        checksum: None,
    };
    let (handle, tx) = downloader.handle(req.id);
    let mut progress = handle.progress.clone();
    let cancel = handle.cancel.clone();
    let download = downloader.download(req, cancel, tx);
    tokio::pin!(download);
    let mut max_connections = 0;
    let summary = timeout(Duration::from_secs(20), async {
        loop {
            tokio::select! {
                result = &mut download => break result.unwrap(),
                changed = progress.changed() => {
                    if changed.is_err() {
                        continue;
                    }
                    max_connections = max_connections.max(progress.borrow().active_connections);
                }
            }
        }
    })
    .await
    .expect("adaptive transfer timed out");
    assert!(
        max_connections > 2,
        "expected connection growth beyond the initial two, saw {max_connections}"
    );
    let bytes = tokio::fs::read(&summary.destination).await.unwrap();
    assert_eq!(bytes, expected_content(0x5EED_1234_ABCD_9876, size));
}
