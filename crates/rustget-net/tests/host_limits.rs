use std::sync::Arc;
use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_net::{DownloadRequest, Downloader, HostLimiter, RequestOptions};
use rustget_testkit::{TestFile, TestServer, TestkitConfig};
use tokio::time::timeout;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shared_limit_caps_requests_across_two_downloads() {
    let server = TestServer::start(
        TestkitConfig::default()
            .with_file(TestFile::new("shared.bin", 4 * 1024 * 1024).throttled(4 * 1024 * 1024)),
    )
    .await
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let cfg = EngineConfig {
        connections: 4,
        max_connections: 8,
        max_connections_per_host: 2,
        min_segment_size: 128 * 1024,
        adaptive_connections: false,
        checkpoint_interval_ms: 100,
        ..EngineConfig::default()
    };
    let limiter = Arc::new(HostLimiter::new(2));
    let downloader = Downloader::new(cfg, RequestOptions::default())
        .unwrap()
        .with_host_limiter(limiter);
    let mut tasks = Vec::new();
    for index in 0..2 {
        let directory = temp.path().join(format!("d{index}"));
        std::fs::create_dir_all(&directory).unwrap();
        let request = DownloadRequest {
            id: Uuid::new_v4(),
            url: url::Url::parse(&server.file_url("shared.bin")).unwrap(),
            mirrors: Vec::new(),
            media_variant: None,
            destination: None,
            directory,
            resume: false,
            overwrite: false,
            checksum: None,
        };
        let downloader = downloader.clone();
        tasks.push(tokio::spawn(async move {
            let (handle, tx) = downloader.handle(request.id);
            downloader
                .download(request, handle.cancel, tx)
                .await
                .unwrap()
        }));
    }
    timeout(Duration::from_secs(30), async {
        for task in tasks {
            task.await.unwrap();
        }
    })
    .await
    .expect("downloads timed out");
    let max = server.max_active();
    assert!(max > 0, "testkit never observed an active body");
    assert!(
        max <= 2,
        "per-host limit was exceeded: {max} concurrent bodies"
    );
}
