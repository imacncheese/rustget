use std::time::Duration;

use rustget_core::EngineConfig;
use rustget_net::{DownloadRequest, Downloader, NetError, RequestOptions};
use rustget_testkit::{TestFile, TestServer, TestkitConfig};
use tokio::time::timeout;
use uuid::Uuid;

fn dash_config() -> TestkitConfig {
    TestkitConfig::default()
        .with_file(TestFile::inline(
            "manifest.mpd",
            "application/dash+xml",
            br#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" mediaPresentationDuration="PT6S">
  <Period>
    <AdaptationSet mimeType="video/mp4" codecs="avc1.4d401f">
      <Representation id="v" bandwidth="900000" width="1280" height="720">
        <SegmentTemplate duration="2" timescale="1" startNumber="1"
          initialization="init-$RepresentationID$.mp4" media="seg-$Number$.m4s"/>
      </Representation>
    </AdaptationSet>
  </Period>
</MPD>"#,
        ))
        .with_file(TestFile::inline("init-v.mp4", "video/mp4", b"INIT"))
        .with_file(TestFile::inline("seg-1.m4s", "video/iso.segment", b"111"))
        .with_file(TestFile::inline("seg-2.m4s", "video/iso.segment", b"222"))
        .with_file(TestFile::inline("seg-3.m4s", "video/iso.segment", b"333"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_duration_template_and_concatenates() {
    let server = TestServer::start(dash_config()).await.unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(EngineConfig::default(), RequestOptions::default()).unwrap();
    let req = DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&server.file_url("manifest.mpd")).unwrap(),
        mirrors: Vec::new(),
        media_variant: None,
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
    .expect("DASH download timed out")
    .unwrap();
    assert_eq!(summary.total_size, 13);
    assert_eq!(summary.segments, 3);
    assert!(summary.destination.ends_with("manifest.mp4"));
    let bytes = tokio::fs::read(&summary.destination).await.unwrap();
    assert_eq!(bytes, b"INIT111222333");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_segment_timeline_with_time_placeholders() {
    let config = TestkitConfig::default()
        .with_file(TestFile::inline(
            "timeline.mpd",
            "application/dash+xml",
            br#"<MPD mediaPresentationDuration="PT6S"><Period><AdaptationSet mimeType="video/mp4">
            <Representation id="v" bandwidth="1000">
              <SegmentTemplate timescale="1" initialization="init.mp4" media="seg-$Time$.m4s">
                <SegmentTimeline><S t="0" d="2" r="2"/></SegmentTimeline>
              </SegmentTemplate>
            </Representation></AdaptationSet></Period></MPD>"#,
        ))
        .with_file(TestFile::inline("init.mp4", "video/mp4", b"INIT"))
        .with_file(TestFile::inline("seg-0.m4s", "video/iso.segment", b"AAA"))
        .with_file(TestFile::inline("seg-2.m4s", "video/iso.segment", b"BBB"))
        .with_file(TestFile::inline("seg-4.m4s", "video/iso.segment", b"CCC"));
    let server = TestServer::start(config).await.unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(EngineConfig::default(), RequestOptions::default()).unwrap();
    let req = DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&server.file_url("timeline.mpd")).unwrap(),
        mirrors: Vec::new(),
        media_variant: None,
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
    .expect("DASH timeline timed out")
    .unwrap();
    assert_eq!(summary.segments, 3);
    assert_eq!(
        tokio::fs::read(&summary.destination).await.unwrap(),
        b"INITAAABBBCCC"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_drm_manifest() {
    let config = TestkitConfig::default().with_file(TestFile::inline(
        "drm.mpd",
        "application/dash+xml",
        br#"<MPD mediaPresentationDuration="PT2S"><Period><AdaptationSet>
        <ContentProtection schemeIdUri="urn:mpeg:dash:mp4protection:2011"/>
        <Representation id="v" bandwidth="1"><SegmentTemplate duration="1" media="s$Number$.m4s"/></Representation>
        </AdaptationSet></Period></MPD>"#,
    ));
    let server = TestServer::start(config).await.unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(EngineConfig::default(), RequestOptions::default()).unwrap();
    let req = DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&server.file_url("drm.mpd")).unwrap(),
        mirrors: Vec::new(),
        media_variant: None,
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
        matches!(error, NetError::Protocol(ref message) if message.contains("DRM")),
        "unexpected error: {error}"
    );
}
