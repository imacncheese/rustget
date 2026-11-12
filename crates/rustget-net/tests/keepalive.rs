use std::collections::HashSet;
use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_net::{DownloadRequest, Downloader, RequestOptions};
use rustget_testkit::{TestServer, TestkitConfig, expected_content};
use tokio::time::timeout;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ranged_workers_reuse_pooled_connections() {
    let size = 2 * 1024 * 1024;
    let server = TestServer::start(TestkitConfig::single(size))
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let connections = 4_u8;
    let downloader = Downloader::new(
        EngineConfig {
            connections,
            max_connections: 8,
            min_segment_size: 128 * 1024,
            ..EngineConfig::default()
        },
        RequestOptions::default(),
    )
    .unwrap();
    let request = DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&server.file_url("file.bin")).unwrap(),
        mirrors: Vec::new(),
        media_variant: None,
        destination: None,
        directory: temp.path().to_path_buf(),
        resume: false,
        overwrite: false,
        checksum: None,
    };
    let (handle, tx) = downloader.handle(request.id);
    let summary = timeout(
        Duration::from_secs(20),
        downloader.download(request, handle.cancel, tx),
    )
    .await
    .expect("download timed out")
    .unwrap();
    assert_eq!(
        tokio::fs::read(summary.destination).await.unwrap(),
        expected_content(0x5EED_1234_ABCD_9876, size)
    );
    let requests = server.requests();
    let peers: HashSet<_> = requests
        .iter()
        .map(|request| request.peer.clone())
        .collect();
    assert!(
        peers.len() <= usize::from(connections) + 1,
        "expected pooled connections, saw {} peers across {} requests",
        peers.len(),
        requests.len()
    );
}
