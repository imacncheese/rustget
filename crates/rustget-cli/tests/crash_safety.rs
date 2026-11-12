use std::path::Path;
use std::time::{Duration, Instant};

use rustget_storage::Journal;
use rustget_testkit::{TestServer, TestkitConfig, expected_content};
use sha2::{Digest, Sha256};

fn downloaded_from_journal(path: &Path) -> u64 {
    Journal::load(path).map_or(0, |state| {
        state
            .segments
            .segments
            .iter()
            .map(|segment| segment.written.saturating_sub(segment.start))
            .sum()
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn kill_9_then_resume_produces_byte_exact_file() {
    let size = 8 * 1024 * 1024;
    let server =
        TestServer::start(TestkitConfig::default().with_file(
            rustget_testkit::TestFile::new("crash.bin", size).throttled(32 * 1024 * 1024),
        ))
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let data_dir = temp.path().join("data");
    let download_dir = temp.path().join("downloads");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&download_dir).unwrap();
    let destination = download_dir.join("crash.bin");
    let journal = download_dir.join("crash.bin.rustget");
    let binary = env!("CARGO_BIN_EXE_rustget");

    let mut child = tokio::process::Command::new(binary)
        .args([
            "--local",
            "--data-dir",
            data_dir.to_str().unwrap(),
            "--download-dir",
            download_dir.to_str().unwrap(),
            "add",
            &server.file_url("crash.bin"),
            "--output",
            destination.to_str().unwrap(),
            "--connections",
            "4",
            "--limit",
            "1",
            "--quiet",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if downloaded_from_journal(&journal) >= 1024 * 1024 {
            break;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("child exited before checkpoint: {status}");
        }
        assert!(Instant::now() < deadline, "no journal checkpoint appeared");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Abrupt termination: no destructor, no graceful checkpoint, no shutdown.
    child.kill().await.unwrap();
    let _ = child.wait().await;
    let partial = downloaded_from_journal(&journal);
    assert!(partial >= 1024 * 1024, "expected durable progress");

    let resumed = tokio::process::Command::new(binary)
        .args([
            "--local",
            "--data-dir",
            data_dir.to_str().unwrap(),
            "--download-dir",
            download_dir.to_str().unwrap(),
            "add",
            &server.file_url("crash.bin"),
            "--output",
            destination.to_str().unwrap(),
            "--connections",
            "4",
            "--quiet",
        ])
        .output()
        .await
        .unwrap();
    assert!(
        resumed.status.success(),
        "resume failed: {}{}",
        String::from_utf8_lossy(&resumed.stdout),
        String::from_utf8_lossy(&resumed.stderr)
    );
    let actual = tokio::fs::read(&destination).await.unwrap();
    let expected = expected_content(0x5EED_1234_ABCD_9876, size);
    assert_eq!(actual.len(), expected.len());
    assert_eq!(Sha256::digest(&actual), Sha256::digest(&expected));
    assert!(!journal.exists());
    assert!(!destination.with_extension("bin.part").exists());
}
