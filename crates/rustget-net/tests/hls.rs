use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_net::{DownloadRequest, Downloader, NetError, RequestOptions};
use rustget_testkit::{TestFile, TestServer, TestkitConfig};
use tokio::time::timeout;
use uuid::Uuid;

fn hls_config() -> TestkitConfig {
    TestkitConfig::default()
        .with_file(TestFile::inline(
            "master.m3u8",
            "application/vnd.apple.mpegurl",
            b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\nmedia.m3u8\n",
        ))
        .with_file(TestFile::inline(
            "media.m3u8",
            "application/vnd.apple.mpegurl",
            b"#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:2\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:2.0,\nseg0.m4s\n#EXTINF:2.0,\nseg1.m4s\n#EXTINF:2.0,\nseg2.m4s\n#EXT-X-ENDLIST\n",
        ))
        .with_file(TestFile::inline("init.mp4", "video/mp4", b"INIT"))
        .with_file(TestFile::inline("seg0.m4s", "video/iso.segment", b"AAA"))
        .with_file(TestFile::inline("seg1.m4s", "video/iso.segment", b"BBB"))
        .with_file(TestFile::inline("seg2.m4s", "video/iso.segment", b"CCC"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_master_playlist_and_concatenates_segments() {
    let server = TestServer::start(hls_config()).await.unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(EngineConfig::default(), RequestOptions::default()).unwrap();
    let req = DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&server.file_url("master.m3u8")).unwrap(),
        mirrors: Vec::new(),
        destination: None,
        directory: temp.path().to_path_buf(),
        resume: false,
        overwrite: false,
        checksum: None,
    };
    let (handle, tx) = downloader.handle(req.id);
    let summary = timeout(
        Duration::from_secs(15),
        downloader.download(req, handle.cancel, tx),
    )
    .await
    .expect("HLS download timed out")
    .unwrap();
    assert_eq!(summary.total_size, 13);
    assert_eq!(summary.segments, 3);
    assert!(summary.destination.ends_with("master.ts"));
    let bytes = tokio::fs::read(&summary.destination).await.unwrap();
    assert_eq!(bytes, b"INITAAABBBCCC");
    let requests = server.requests();
    assert!(
        requests
            .iter()
            .any(|entry| entry.path.ends_with("master.m3u8"))
    );
    assert!(
        requests
            .iter()
            .any(|entry| entry.path.ends_with("media.m3u8"))
    );
    assert!(
        requests
            .iter()
            .any(|entry| entry.path.ends_with("seg2.m4s"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_encrypted_hls_cleanly() {
    let config = TestkitConfig::default().with_file(TestFile::inline(
        "enc.m3u8",
        "application/vnd.apple.mpegurl",
        b"#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXTINF:1,\nseg.m4s\n#EXT-X-ENDLIST\n",
    ));
    let server = TestServer::start(config).await.unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(EngineConfig::default(), RequestOptions::default()).unwrap();
    let req = DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&server.file_url("enc.m3u8")).unwrap(),
        mirrors: Vec::new(),
        destination: None,
        directory: temp.path().to_path_buf(),
        resume: false,
        overwrite: false,
        checksum: None,
    };
    let (handle, tx) = downloader.handle(req.id);
    let error = downloader
        .download(req, handle.cancel, tx)
        .await
        .unwrap_err();
    assert!(
        matches!(error, NetError::Protocol(ref message) if message.contains("encrypted")),
        "unexpected error: {error}"
    );
}
