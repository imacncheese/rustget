use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_net::{DownloadRequest, Downloader, RequestOptions};
use rustget_testkit::{FtpFixtureConfig, FtpTestServer, expected_content};
use tokio::time::timeout;
use uuid::Uuid;

fn request(url: String, temp: &std::path::Path) -> DownloadRequest {
    DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&url).unwrap(),
        mirrors: Vec::new(),
        destination: None,
        directory: temp.to_path_buf(),
        resume: true,
        overwrite: false,
        checksum: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_ftp_file_and_resumes_with_rest() {
    let size = 1024 * 1024;
    let content = expected_content(0x5EED_1234_ABCD_9876, size);
    let server = FtpTestServer::start(FtpFixtureConfig::new(content.clone()))
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(EngineConfig::default(), RequestOptions::default()).unwrap();
    let req = request(server.url(), temp.path());
    let (handle, tx) = downloader.handle(req.id);
    let summary = timeout(
        Duration::from_secs(20),
        downloader.download(req, handle.cancel, tx),
    )
    .await
    .expect("FTP download timed out")
    .unwrap();
    assert_eq!(summary.total_size, size);
    assert_eq!(
        tokio::fs::read(&summary.destination).await.unwrap(),
        content
    );

    // Resume from a prefix. The fixture applies REST and therefore the output
    // must not duplicate the already-present bytes.
    let resume_dir = temp.path().join("resume");
    std::fs::create_dir_all(&resume_dir).unwrap();
    std::fs::write(resume_dir.join("file.bin.part"), &content[..384 * 1024]).unwrap();
    let req = request(server.url(), &resume_dir);
    let (handle, tx) = downloader.handle(req.id);
    let summary = timeout(
        Duration::from_secs(20),
        downloader.download(req, handle.cancel, tx),
    )
    .await
    .expect("FTP resume timed out")
    .unwrap();
    assert_eq!(
        tokio::fs::read(&summary.destination).await.unwrap(),
        content
    );
    server.shutdown().await;
}
