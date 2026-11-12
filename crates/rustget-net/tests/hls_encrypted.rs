use std::time::Duration;

use aes::Aes128;
use cbc::Encryptor;
use cipher::block_padding::Pkcs7;
use cipher::{BlockEncryptMut, KeyIvInit};
use rustget_core::EngineConfig;
use rustget_net::{DownloadRequest, Downloader, RequestOptions};
use rustget_testkit::{TestFile, TestServer, TestkitConfig};
use tokio::time::timeout;
use uuid::Uuid;

const KEY: [u8; 16] = [0x11; 16];
const IV: [u8; 16] = [0x22; 16];

fn encrypt(plain: &[u8]) -> Vec<u8> {
    let mut buffer = plain.to_vec();
    let length = buffer.len();
    buffer.resize(length + 16, 0);
    Encryptor::<Aes128>::new(&KEY.into(), &IV.into())
        .encrypt_padded_mut::<Pkcs7>(&mut buffer, length)
        .unwrap()
        .to_vec()
}

fn encrypted_config() -> (TestkitConfig, Vec<u8>) {
    let plain0 = b"first segment plaintext";
    let plain1 = b"second segment plaintext";
    let mut expected = Vec::new();
    expected.extend_from_slice(plain0);
    expected.extend_from_slice(plain1);
    let playlist = format!(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:2\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x{}\n#EXTINF:2.0,\nseg0.m4s\n#EXTINF:2.0,\nseg1.m4s\n#EXT-X-ENDLIST\n",
        "22".repeat(16)
    );
    let config = TestkitConfig::default()
        .with_file(TestFile::inline(
            "enc.m3u8",
            "application/vnd.apple.mpegurl",
            playlist.into_bytes(),
        ))
        .with_file(TestFile::inline(
            "key.bin",
            "application/octet-stream",
            KEY.to_vec(),
        ))
        .with_file(TestFile::inline(
            "seg0.m4s",
            "video/iso.segment",
            encrypt(plain0),
        ))
        .with_file(TestFile::inline(
            "seg1.m4s",
            "video/iso.segment",
            encrypt(plain1),
        ));
    (config, expected)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn decrypts_aes128_hls_segments() {
    let (config, expected) = encrypted_config();
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
    let summary = timeout(
        Duration::from_secs(15),
        downloader.download(req, handle.cancel, tx),
    )
    .await
    .expect("encrypted HLS timed out")
    .unwrap();
    let bytes = tokio::fs::read(&summary.destination).await.unwrap();
    assert_eq!(bytes, expected);
    assert!(
        server
            .requests()
            .iter()
            .any(|entry| entry.path.ends_with("key.bin"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_invalid_aes_padding() {
    let playlist = format!(
        "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x{}\n#EXTINF:1,\nseg.m4s\n#EXT-X-ENDLIST\n",
        "22".repeat(16)
    );
    let config = TestkitConfig::default()
        .with_file(TestFile::inline(
            "bad.m3u8",
            "application/vnd.apple.mpegurl",
            playlist.into_bytes(),
        ))
        .with_file(TestFile::inline(
            "key.bin",
            "application/octet-stream",
            KEY.to_vec(),
        ))
        .with_file(TestFile::inline(
            "seg.m4s",
            "video/iso.segment",
            vec![0_u8; 16],
        ));
    let server = TestServer::start(config).await.unwrap();
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(EngineConfig::default(), RequestOptions::default()).unwrap();
    let req = DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&server.file_url("bad.m3u8")).unwrap(),
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
    assert!(format!("{error}").contains("padding"), "{error}");
}
