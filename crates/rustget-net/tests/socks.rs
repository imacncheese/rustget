use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_net::{DownloadRequest, Downloader, RequestOptions};
use rustget_testkit::socks::SocksServer;
use rustget_testkit::{TestServer, TestkitConfig, expected_content};
use tokio::time::timeout;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_through_a_socks5_proxy() {
    let size = 256 * 1024;
    let server = TestServer::start(TestkitConfig::single(size))
        .await
        .unwrap();
    let socks = SocksServer::start().await.unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(
        EngineConfig::default(),
        RequestOptions {
            proxy: Some(socks.url()),
            ..RequestOptions::default()
        },
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
    .expect("SOCKS download timed out")
    .unwrap();
    assert_eq!(
        tokio::fs::read(summary.destination).await.unwrap(),
        expected_content(0x5EED_1234_ABCD_9876, size)
    );
}
