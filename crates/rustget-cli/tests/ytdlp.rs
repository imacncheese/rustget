use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_queue::{DownloadManager, DownloadStatus, ManagerConfig, ManagerEvent, NewDownload};
use rustget_testkit::{TestServer, TestkitConfig, expected_content};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ytdlp_resolution_replaces_the_download_url() {
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
        "ytdlp-stub".to_string(),
        server.file_url("file.bin"),
    ])
    .unwrap();
    manager.set_setting("ytdlp.command", &command).unwrap();
    let mut events = manager.subscribe();
    let record = manager
        .add(NewDownload {
            url: "https://original.invalid/video".into(),
            use_ytdlp: true,
            ..NewDownload::default()
        })
        .await
        .unwrap();
    assert!(record.url.starts_with(&server.file_url("file.bin")));
    assert!(record.url.contains("resolved=1"));
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
async fn ytdlp_without_configuration_fails() {
    let directory = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(ManagerConfig {
        data_dir: directory.path().join("data"),
        download_dir: directory.path().join("downloads"),
        engine: EngineConfig::default(),
        max_concurrent: 1,
    })
    .unwrap();
    let result = manager
        .add(NewDownload {
            url: "https://original.invalid/video".into(),
            use_ytdlp: true,
            start: false,
            ..NewDownload::default()
        })
        .await;
    assert!(result.is_err());
}
