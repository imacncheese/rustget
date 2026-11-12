use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_net::{DownloadRequest, Downloader, RequestOptions};
use rustget_testkit::{TestFile, TestServer, TestkitConfig, expected_content};
use tokio::time::timeout;
use uuid::Uuid;

fn request(url: String, temp: &std::path::Path) -> DownloadRequest {
    DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&url).unwrap(),
        mirrors: Vec::new(),
        media_variant: None,
        destination: None,
        directory: temp.to_path_buf(),
        resume: false,
        overwrite: false,
        checksum: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn digest_challenge_is_answered_and_download_completes() {
    let size = 512 * 1024;
    let server = TestServer::start(
        TestkitConfig::default().with_file(
            TestFile::new("digest.bin", size)
                .throttled(8 * 1024 * 1024)
                .digest("user", "pass", "rustget"),
        ),
    )
    .await
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let options = RequestOptions {
        digest_auth: Some(("user".into(), "pass".into())),
        ..RequestOptions::default()
    };
    let downloader = Downloader::new(
        EngineConfig {
            connections: 2,
            min_segment_size: 128 * 1024,
            ..EngineConfig::default()
        },
        options,
    )
    .unwrap();
    let req = request(server.file_url("digest.bin"), temp.path());
    let (handle, tx) = downloader.handle(req.id);
    let summary = timeout(
        Duration::from_secs(20),
        downloader.download(req, handle.cancel, tx),
    )
    .await
    .expect("digest download timed out")
    .unwrap();
    assert_eq!(
        tokio::fs::read(summary.destination).await.unwrap(),
        expected_content(0x5EED_1234_ABCD_9876, size)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn digest_challenge_without_credentials_fails() {
    let server = TestServer::start(
        TestkitConfig::default()
            .with_file(TestFile::new("digest.bin", 128 * 1024).digest("user", "pass", "rustget")),
    )
    .await
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(EngineConfig::default(), RequestOptions::default()).unwrap();
    let req = request(server.file_url("digest.bin"), temp.path());
    let (handle, tx) = downloader.handle(req.id);
    assert!(downloader.download(req, handle.cancel, tx).await.is_err());
}
