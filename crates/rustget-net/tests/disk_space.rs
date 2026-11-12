use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_net::{DownloadRequest, Downloader, NetError, RequestOptions};
use rustget_testkit::{TestServer, TestkitConfig};
use tokio::time::timeout;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn insufficient_reserved_space_fails_before_writing() {
    let server = TestServer::start(TestkitConfig::single(128 * 1024))
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(
        EngineConfig {
            reserve_bytes: u64::MAX,
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
    let result = timeout(
        Duration::from_secs(20),
        downloader.download(request, handle.cancel, tx),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(NetError::InsufficientSpace { .. })));
    assert!(!temp.path().join("file.bin").exists());
}
