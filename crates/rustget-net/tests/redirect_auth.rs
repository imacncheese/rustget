use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_net::{DownloadRequest, Downloader, RequestOptions};
use rustget_testkit::{TestFile, TestServer, TestkitConfig, expected_content};
use tokio::time::timeout;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cross_host_redirect_drops_authorization() {
    let size = 128 * 1024;
    let target_server = TestServer::start(TestkitConfig::single(size))
        .await
        .unwrap();
    let target = target_server
        .file_url("file.bin")
        .replace("127.0.0.1", "localhost");
    let redirect_server = TestServer::start(
        TestkitConfig::default().with_file(TestFile::new("redir.bin", 0).redirect_to(target)),
    )
    .await
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(
        EngineConfig::default(),
        RequestOptions {
            bearer_token: Some("super-secret".into()),
            ..RequestOptions::default()
        },
    )
    .unwrap();
    let req = DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&redirect_server.go_url("redir.bin")).unwrap(),
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
    .expect("redirect download timed out")
    .unwrap();
    assert_eq!(
        tokio::fs::read(summary.destination).await.unwrap(),
        expected_content(0x5EED_1234_ABCD_9876, size)
    );
    let requests = target_server.requests();
    assert!(!requests.is_empty());
    assert!(requests.iter().all(|request| !request.has_authorization));
}
