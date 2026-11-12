use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_queue::{DownloadManager, DownloadStatus, ManagerConfig, ManagerEvent, NewDownload};
use rustget_testkit::{TestFile, TestServer, TestkitConfig, expected_content};

async fn wait_complete(manager: &DownloadManager, id: uuid::Uuid) {
    let mut events = manager.subscribe();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(ManagerEvent::Finished {
                id: event_id,
                status,
            }) = events.recv().await
            {
                if event_id == id {
                    assert_eq!(status, DownloadStatus::Completed);
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cookie_jar_persists_a_session_cookie() {
    let size = 128 * 1024;
    let server = TestServer::start(
        TestkitConfig::default()
            .with_file(
                TestFile::new("login.bin", size).set_cookie("session=abc; Path=/; Max-Age=3600"),
            )
            .with_file(TestFile::new("protected.bin", size).require_cookie("session=abc")),
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
    let first = manager
        .add(NewDownload {
            url: server.file_url("login.bin"),
            use_cookie_jar: true,
            ..NewDownload::default()
        })
        .await
        .unwrap();
    wait_complete(&manager, first.id).await;
    let second = manager
        .add(NewDownload {
            url: server.file_url("protected.bin"),
            use_cookie_jar: true,
            ..NewDownload::default()
        })
        .await
        .unwrap();
    wait_complete(&manager, second.id).await;
    let stored = manager.get(second.id).unwrap();
    assert_eq!(
        std::fs::read(&stored.destination).unwrap(),
        expected_content(0x5EED_1234_ABCD_9876, size)
    );
}
