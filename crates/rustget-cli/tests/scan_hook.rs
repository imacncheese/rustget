use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_queue::{DownloadManager, DownloadStatus, ManagerConfig, ManagerEvent, NewDownload};
use rustget_testkit::{TestServer, TestkitConfig, expected_content};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completed_download_runs_the_configured_scanner() {
    let size = 256 * 1024;
    let server = TestServer::start(TestkitConfig::single(size))
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
    let command = serde_json::to_string(&vec![
        env!("CARGO_BIN_EXE_rustget").to_string(),
        "scan".to_string(),
    ])
    .unwrap();
    manager
        .set_setting("antivirus.scan_command", &command)
        .unwrap();
    let mut events = manager.subscribe();
    let record = manager
        .add(NewDownload {
            url: server.file_url("file.bin"),
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
async fn failed_scanner_marks_the_download_failed() {
    let size = 64 * 1024;
    let server = TestServer::start(TestkitConfig::single(size))
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
    let failing = serde_json::to_string(&vec![
        env!("CARGO_BIN_EXE_rustget").to_string(),
        "scan-does-not-exist".to_string(),
    ])
    .unwrap();
    manager
        .set_setting("antivirus.scan_command", &failing)
        .unwrap();
    let mut events = manager.subscribe();
    let record = manager
        .add(NewDownload {
            url: server.file_url("file.bin"),
            ..NewDownload::default()
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(ManagerEvent::Finished { id, status }) = events.recv().await {
                if id == record.id {
                    assert_eq!(status, DownloadStatus::Failed);
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
    let stored = manager.get(record.id).unwrap();
    assert!(stored.error.unwrap().contains("antivirus scan"));
}
