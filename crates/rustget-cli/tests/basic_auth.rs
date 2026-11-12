use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_queue::{DownloadManager, DownloadStatus, ManagerConfig, ManagerEvent, NewDownload};
use rustget_testkit::{TestFile, TestServer, TestkitConfig, expected_content};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn basic_auth_is_applied_from_the_download_record() {
    let size = 128 * 1024;
    let server = TestServer::start(
        TestkitConfig::default()
            .with_file(TestFile::new("file.bin", size).with_basic_auth("user", "pass")),
    )
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
            url: server.file_url("file.bin"),
            basic_auth: Some(("user".into(), "pass".into())),
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
    let stored = manager.get(record.id).unwrap();
    assert_eq!(
        std::fs::read(&stored.destination).unwrap(),
        expected_content(0x5EED_1234_ABCD_9876, size)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn basic_auth_without_credentials_fails_at_add() {
    let server = TestServer::start(
        TestkitConfig::default()
            .with_file(TestFile::new("file.bin", 64 * 1024).with_basic_auth("user", "pass")),
    )
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
    assert!(
        manager
            .add(NewDownload {
                url: server.file_url("file.bin"),
                ..NewDownload::default()
            })
            .await
            .is_err()
    );
}
