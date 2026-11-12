use std::io::Write;
use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_queue::{DownloadManager, DownloadStatus, ManagerConfig, ManagerEvent, NewDownload};
use rustget_testkit::{TestFile, TestServer, TestkitConfig};

fn archive() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut bytes));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("hello.txt", options).unwrap();
        writer.write_all(b"hello from the archive").unwrap();
        writer.finish().unwrap();
    }
    bytes
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completed_zip_is_extracted() {
    let server = TestServer::start(TestkitConfig::default().with_file(TestFile::inline(
        "archive.zip",
        "application/zip",
        archive(),
    )))
    .await
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(ManagerConfig {
        data_dir: directory.path().join("data"),
        download_dir: directory.path().join("downloads"),
        engine: EngineConfig::default(),
        max_concurrent: 1,
    })
    .unwrap();
    let mut events = manager.subscribe();
    let record = manager
        .add(NewDownload {
            url: server.file_url("archive.zip"),
            extract: true,
            ..NewDownload::default()
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(ManagerEvent::Finished { id, status }) = events.recv().await {
                if id == record.id {
                    assert_eq!(status, DownloadStatus::Completed);
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
    let extracted = directory
        .path()
        .join("downloads")
        .join("archive")
        .join("hello.txt");
    assert_eq!(
        std::fs::read_to_string(extracted).unwrap(),
        "hello from the archive"
    );
}
