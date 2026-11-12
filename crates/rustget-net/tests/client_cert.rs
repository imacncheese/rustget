use std::sync::Arc;
use std::time::Duration;

use rcgen::{BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair};
use rustget_core::EngineConfig;
use rustget_net::{DownloadRequest, Downloader, RequestOptions};
use rustls::RootCertStore;
use rustls::ServerConfig;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::WebPkiClientVerifier;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use uuid::Uuid;

struct Material {
    server_config: Arc<ServerConfig>,
    client_pem: Vec<u8>,
}

fn material() -> Material {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::new(Vec::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca_params, ca_key);

    let server_key = KeyPair::generate().unwrap();
    let mut server_params =
        CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()]).unwrap();
    server_params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ServerAuth);
    let server_cert = server_params.signed_by(&server_key, &issuer).unwrap();

    let client_key = KeyPair::generate().unwrap();
    let mut client_params = CertificateParams::new(Vec::new()).unwrap();
    client_params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ClientAuth);
    let client_cert = client_params.signed_by(&client_key, &issuer).unwrap();
    let client_pem = format!("{}{}", client_cert.pem(), client_key.serialize_pem());

    let mut roots = RootCertStore::empty();
    roots.add(ca_cert.der().clone()).unwrap();
    let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
        .build()
        .unwrap();
    let config = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            vec![server_cert.der().clone(), ca_cert.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(server_key.serialize_der())),
        )
        .unwrap();
    Material {
        server_config: Arc::new(config),
        client_pem: client_pem.into_bytes(),
    }
}

async fn start_server(config: Arc<ServerConfig>) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let acceptor = TlsAcceptor::from(config);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(stream).await else {
                    return;
                };
                let mut buffer = [0_u8; 4096];
                let Ok(read) = tls.read(&mut buffer).await else {
                    return;
                };
                let head = String::from_utf8_lossy(&buffer[..read]);
                let body = b"CLIENT-CERT-OK";
                let is_head = head.starts_with("HEAD ");
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = tls.write_all(response.as_bytes()).await;
                if !is_head {
                    let _ = tls.write_all(body).await;
                }
                let _ = tls.shutdown().await;
            });
        }
    });
    addr
}

fn request(url: String, temp: &std::path::Path) -> DownloadRequest {
    DownloadRequest {
        id: Uuid::new_v4(),
        url: url::Url::parse(&url).unwrap(),
        mirrors: Vec::new(),
        media_variant: None,
        destination: None,
        directory: temp.to_path_buf(),
        resume: false,
        overwrite: false,
        checksum: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mutual_tls_client_certificate_is_used() {
    let material = material();
    let addr = start_server(Arc::clone(&material.server_config)).await;
    let temp = tempfile::tempdir().unwrap();
    let options = RequestOptions {
        client_identity_pem: Some(material.client_pem),
        accept_invalid_certs: true,
        ..RequestOptions::default()
    };
    let downloader = Downloader::new(EngineConfig::default(), options).unwrap();
    let req = request(
        format!("https://127.0.0.1:{}/file.bin", addr.port()),
        temp.path(),
    );
    let (handle, tx) = downloader.handle(req.id);
    let summary = tokio::time::timeout(
        Duration::from_secs(20),
        downloader.download(req, handle.cancel, tx),
    )
    .await
    .expect("mTLS download timed out")
    .unwrap();
    assert_eq!(
        tokio::fs::read(summary.destination).await.unwrap(),
        b"CLIENT-CERT-OK"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mutual_tls_without_certificate_is_rejected() {
    let material = material();
    let addr = start_server(Arc::clone(&material.server_config)).await;
    let temp = tempfile::tempdir().unwrap();
    let downloader = Downloader::new(
        EngineConfig::default(),
        RequestOptions {
            accept_invalid_certs: true,
            ..RequestOptions::default()
        },
    )
    .unwrap();
    let req = request(
        format!("https://127.0.0.1:{}/file.bin", addr.port()),
        temp.path(),
    );
    let (handle, tx) = downloader.handle(req.id);
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        downloader.download(req, handle.cancel, tx),
    )
    .await
    .unwrap();
    assert!(result.is_err());
}
