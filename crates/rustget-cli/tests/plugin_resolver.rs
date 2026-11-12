use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_queue::{DownloadManager, DownloadStatus, ManagerConfig, ManagerEvent, NewDownload};
use rustget_testkit::{TestServer, TestkitConfig, expected_content};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn named_resolver_plugin_replaces_the_url() {
    let size = 128 * 1024;
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
    let registry = serde_json::json!({
        "test": [
            env!("CARGO_BIN_EXE_rustget"),
            "ytdlp-stub",
            server.file_url("file.bin"),
        ]
    });
    manager
        .set_setting(
            "plugins.resolvers",
            &serde_json::to_string(&registry).unwrap(),
        )
        .unwrap();
    let mut events = manager.subscribe();
    let record = manager
        .add(NewDownload {
            url: "https://original.invalid/video".into(),
            resolver: Some("test".into()),
            ..NewDownload::default()
        })
        .await
        .unwrap();
    assert_eq!(record.resolver.as_deref(), Some("test"));
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
async fn unknown_resolver_plugin_fails() {
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
            resolver: Some("missing".into()),
            start: false,
            ..NewDownload::default()
        })
        .await;
    assert!(result.is_err());
}
