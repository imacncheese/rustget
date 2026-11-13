use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use h2::server;
use rcgen::{CertificateParams, KeyPair};
use rustget_core::EngineConfig;
use rustget_net::{DownloadRequest, Downloader, RequestOptions};
use rustget_testkit::expected_content;
use rustls::ServerConfig;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use uuid::Uuid;

fn server_config() -> (Arc<ServerConfig>, Vec<u8>) {
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()]).unwrap();
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    let cert = params.self_signed(&key).unwrap();
    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )
        .unwrap();
    config.alpn_protocols = vec![b"h2".to_vec()];
    (Arc::new(config), cert.der().to_vec())
}

async fn start_h2_server(size: u64) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (config, _) = server_config();
    let acceptor = TlsAcceptor::from(config);
    let body = expected_content(0x5EED_1234_ABCD_9876, size);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            let body = body.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else {
                    return;
                };
                let Ok(mut connection) = server::handshake(tls).await else {
                    return;
                };
                while let Some(Ok((request, mut respond))) = connection.accept().await {
                    let head = request.method() == http::Method::HEAD;
                    let response = http::Response::builder()
                        .status(200)
                        .header("content-length", body.len())
                        .body(())
                        .unwrap();
                    let Ok(mut send) = respond.send_response(response, false) else {
                        return;
                    };
                    let payload = if head {
                        Bytes::new()
                    } else {
                        Bytes::copy_from_slice(&body)
                    };
                    let _ = send.send_data(payload, true);
                }
            });
        }
    });
    addr
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_over_http2_tls() {
    let size = 128 * 1024_u64;
    let addr = start_h2_server(size).await;
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(
        EngineConfig::default(),
        RequestOptions {
            accept_invalid_certs: true,
            ..RequestOptions::default()
        },
    )
    .unwrap();
    let request = DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&format!("https://127.0.0.1:{}/file.bin", addr.port())).unwrap(),
        mirrors: Vec::new(),
        media_variant: None,
        destination: None,
        directory: temp.path().to_path_buf(),
        resume: false,
        overwrite: false,
        checksum: None,
    };
    let (handle, tx) = downloader.handle(request.id);
    let summary = tokio::time::timeout(
        Duration::from_secs(20),
        downloader.download(request, handle.cancel, tx),
    )
    .await
    .expect("h2 download timed out")
    .unwrap();
    assert_eq!(
        tokio::fs::read(summary.destination).await.unwrap(),
        expected_content(0x5EED_1234_ABCD_9876, size)
    );
}
