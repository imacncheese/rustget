use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_ipc::{ApiConfig, LogBuffer, router};
use rustget_queue::{Category, DownloadManager, ManagerConfig, PowerAction, Schedule};
use rustget_testkit::{TestFile, TestServer, TestkitConfig};
use tokio::net::TcpListener;

async fn start_api() -> (
    String,
    String,
    Arc<DownloadManager>,
    TestServer,
    tempfile::TempDir,
    Arc<LogBuffer>,
) {
    let origin = TestServer::start(rustget_testkit::TestkitConfig::single(256 * 1024))
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(ManagerConfig {
        data_dir: temp.path().join("data"),
        download_dir: temp.path().join("downloads"),
        engine: EngineConfig {
            connections: 2,
            min_segment_size: 32 * 1024,
            ..EngineConfig::default()
        },
        max_concurrent: 2,
    })
    .unwrap();
    let token = "test-token-0123456789abcdef0123456789abcdef".to_string();
    let logs = Arc::new(LogBuffer::new(20));
    let mut config = ApiConfig::loopback(token.clone());
    config.logs = Some(Arc::clone(&logs));
    let app = router(Arc::clone(&manager), &config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (format!("http://{addr}"), token, manager, origin, temp, logs)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serves_captured_logs() {
    let (base, token, _manager, _origin, _temp, logs) = start_api().await;
    logs.push("WARN", "rustget.test", "captured message".into());
    let body: serde_json::Value = reqwest::Client::new()
        .get(format!("{base}/api/v1/logs?limit=10"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["entries"][0]["level"], "WARN");
    assert_eq!(body["entries"][0]["message"], "captured message");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requires_bearer_and_serves_the_ui() {
    let (base, token, _manager, _origin, _temp, _logs) = start_api().await;
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .get(format!("{base}/api/v1/downloads"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let authorized = client
        .get(format!("{base}/api/v1/downloads"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(authorized.status(), 200);
    assert!(
        authorized
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );
    let page = client.get(format!("{base}/")).send().await.unwrap();
    assert_eq!(page.status(), 200);
    assert!(page.text().await.unwrap().contains("RustGet"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn add_list_and_delete_download() {
    let (base, token, manager, origin, _temp, _logs) = start_api().await;
    let client = reqwest::Client::new();
    let probed: serde_json::Value = client
        .post(format!("{base}/api/v1/probe"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"url": origin.file_url("file.bin")}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(probed["filename"], "file.bin");
    assert_eq!(probed["total_size"], 256 * 1024);
    assert_eq!(probed["range_supported"], true);

    let media_origin = TestServer::start(TestkitConfig::default().with_file(TestFile::inline(
        "master.m3u8",
        "application/vnd.apple.mpegurl",
        b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=400000,RESOLUTION=320x180\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\nhigh.m3u8\n",
    )))
    .await
    .unwrap();
    let media: serde_json::Value = client
        .post(format!("{base}/api/v1/media/probe"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"url": media_origin.file_url("master.m3u8")}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(media["variants"].as_array().unwrap().len(), 2);
    assert_eq!(media["variants"][0]["label"], "320x180");

    let response = client
        .post(format!("{base}/api/v1/downloads"))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "url": origin.file_url("file.bin"),
            "connections": 2,
            "category": "Test"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let record: serde_json::Value = response.json().await.unwrap();
    let id = record["id"].as_str().unwrap().to_string();
    assert_eq!(record["filename"], "file.bin");

    let patched: serde_json::Value = client
        .patch(format!("{base}/api/v1/downloads/{id}"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"priority": 7, "tags": ["linux", "iso"]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(patched["priority"], 7);
    assert_eq!(patched["tags"][0], "linux");
    assert_eq!(patched["tags"][1], "iso");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let response = client
            .get(format!("{base}/api/v1/downloads/{id}"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = response.json().await.unwrap();
        if body["download"]["status"] == "completed" {
            assert!(
                std::path::Path::new(body["download"]["destination"].as_str().unwrap()).exists()
            );
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "download did not finish"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let duplicate = client
        .post(format!("{base}/api/v1/downloads"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"url": origin.file_url("file.bin")}))
        .send()
        .await
        .unwrap();
    assert_eq!(duplicate.status(), 409);

    let exported: serde_json::Value = client
        .get(format!("{base}/api/v1/export"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(exported["downloads"].as_array().unwrap().len(), 1);
    let imported: serde_json::Value = client
        .post(format!("{base}/api/v1/import"))
        .bearer_auth(&token)
        .json(&serde_json::json!({"downloads": exported["downloads"]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(imported["imported"], 1);

    let stats: serde_json::Value = client
        .get(format!("{base}/api/v1/stats"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(stats["completed"], 1);

    let removed = client
        .delete(format!("{base}/api/v1/downloads/{id}?delete_files=true"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(removed.status(), 204);
    assert!(manager.get(uuid::Uuid::parse_str(&id).unwrap()).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exports_and_imports_settings_categories_and_schedules() {
    let (base, token, manager, _origin, _temp, _logs) = start_api().await;
    manager.set_setting("theme", "dark").unwrap();
    manager
        .upsert_category(&Category {
            name: "TestCat".into(),
            directory: "D:/test".into(),
            extensions: vec!["rst".into()],
        })
        .unwrap();
    manager
        .replace_schedule(&Schedule {
            id: uuid::Uuid::new_v4(),
            name: "night".into(),
            enabled: true,
            days: 0b0001_1111,
            start_min: Some(60),
            stop_min: Some(360),
            action: PowerAction::None,
        })
        .unwrap();
    let client = reqwest::Client::new();
    let exported: serde_json::Value = client
        .get(format!("{base}/api/v1/export"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(exported["settings"]["theme"], "dark");
    assert!(
        exported["categories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "TestCat")
    );
    assert_eq!(exported["schedules"].as_array().unwrap().len(), 1);

    let imported: serde_json::Value = client
        .post(format!("{base}/api/v1/import"))
        .bearer_auth(&token)
        .json(&exported)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(imported["settings"], 1);
    assert_eq!(
        imported["categories"],
        exported["categories"].as_array().unwrap().len()
    );
    assert_eq!(imported["schedules"], 1);
    assert_eq!(
        manager.database().get_setting("theme").unwrap().as_deref(),
        Some("dark")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_non_loopback_host_header() {
    let (base, token, _manager, _origin, _temp, _logs) = start_api().await;
    let response = reqwest::Client::new()
        .get(format!("{base}/api/v1/downloads"))
        .bearer_auth(&token)
        .header("Host", "rebind.example")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_cross_origin_mutation() {
    let (base, token, _manager, _origin, _temp, _logs) = start_api().await;
    let response = reqwest::Client::new()
        .post(format!("{base}/api/v1/downloads"))
        .bearer_auth(&token)
        .header("Origin", "https://evil.example")
        .json(&serde_json::json!({"url": "https://example.test/x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
}
