use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_queue::{DownloadManager, DownloadStatus, ManagerConfig, ManagerEvent, NewDownload};
use rustget_testkit::{TestServer, TestkitConfig, expected_content};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completed_download_runs_the_post_action() {
    let size = 128 * 1024;
    let server = TestServer::start(TestkitConfig::single(size))
        .await
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("post-action.out");
    let manager = DownloadManager::new(ManagerConfig {
        data_dir: directory.path().join("data"),
        download_dir: directory.path().join("downloads"),
        engine: EngineConfig::default(),
        max_concurrent: 1,
    })
    .unwrap();
    let command = serde_json::to_string(&vec![
        env!("CARGO_BIN_EXE_rustget").to_string(),
        "post-stub".to_string(),
        marker.to_string_lossy().to_string(),
    ])
    .unwrap();
    manager
        .set_setting("post_action.command", &command)
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
    assert_eq!(
        std::fs::read(marker).unwrap(),
        expected_content(0x5EED_1234_ABCD_9876, size)
    );
}
