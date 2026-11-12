use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_net::{DownloadRequest, Downloader, RequestOptions};
use rustget_testkit::{ProxyServer, TestFile, TestServer, TestkitConfig, expected_content};
use tokio::time::timeout;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_through_an_explicit_http_proxy() {
    let size = 512 * 1024;
    let origin = TestServer::start(
        TestkitConfig::default()
            .with_file(TestFile::new("proxied.bin", size).throttled(8 * 1024 * 1024)),
    )
    .await
    .unwrap();
    let proxy = ProxyServer::start().await.unwrap();
    let temp = tempfile::tempdir().unwrap();
    let options = RequestOptions {
        proxy: Some(proxy.url()),
        ..RequestOptions::default()
    };
    let downloader = Downloader::new(
        EngineConfig {
            connections: 1,
            max_connections: 1,
            min_segment_size: size,
            adaptive_connections: false,
            ..EngineConfig::default()
        },
        options,
    )
    .unwrap();
    let req = DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&origin.file_url("proxied.bin")).unwrap(),
        mirrors: Vec::new(),
        media_variant: None,
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
    .expect("proxy download timed out")
    .unwrap();
    assert_eq!(
        tokio::fs::read(summary.destination).await.unwrap(),
        expected_content(0x5EED_1234_ABCD_9876, size)
    );
    assert!(proxy.requests() > 0, "proxy never saw a request");
    proxy.shutdown().await;
}
