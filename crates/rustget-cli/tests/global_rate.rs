use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_queue::{DownloadManager, DownloadStatus, ManagerConfig, ManagerEvent, NewDownload};
use rustget_testkit::{TestFile, TestServer, TestkitConfig};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn global_rate_limit_is_shared_by_concurrent_downloads() {
    let size = 256 * 1024;
    let server = TestServer::start(
        TestkitConfig::default()
            .with_file(TestFile::new("a.bin", size))
            .with_file(TestFile::new("b.bin", size)),
    )
    .await
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(ManagerConfig {
        data_dir: directory.path().join("data"),
        download_dir: directory.path().join("downloads"),
        engine: EngineConfig {
            connections: 1,
            max_speed_bytes_per_sec: Some(size),
            ..EngineConfig::default()
        },
        max_concurrent: 2,
    })
    .unwrap();
    let mut events = manager.subscribe();
    let first = manager
        .add(NewDownload {
            url: server.file_url("a.bin"),
            allow_duplicate: true,
            ..NewDownload::default()
        })
        .await
        .unwrap();
    let second = manager
        .add(NewDownload {
            url: server.file_url("b.bin"),
            allow_duplicate: true,
            ..NewDownload::default()
        })
        .await
        .unwrap();
    let started = tokio::time::Instant::now();
    let mut finished = 0;
    tokio::time::timeout(Duration::from_secs(20), async {
        while finished < 2 {
            if let Ok(ManagerEvent::Finished { id, status }) = events.recv().await {
                if id == first.id || id == second.id {
                    assert_eq!(status, DownloadStatus::Completed);
                    finished += 1;
                }
            }
        }
    })
    .await
    .unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(600),
        "shared limiter did not throttle: {:?}",
        started.elapsed()
    );
}
