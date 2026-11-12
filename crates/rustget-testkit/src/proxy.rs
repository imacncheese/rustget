//! Minimal forward HTTP proxy fixture for engine tests. It accepts absolute-form
//! requests, rewrites them to origin-form and pipes the connection.
#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

pub struct ProxyServer {
    pub addr: SocketAddr,
    requests: Arc<AtomicU64>,
    shutdown: Option<oneshot::Sender<()>>,
}

impl ProxyServer {
    pub async fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let requests = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&requests);
        let (tx, mut rx) = oneshot::channel();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut rx => break,
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { break };
                        let counter = Arc::clone(&counter);
                        tokio::spawn(async move {
                            let _ = handle(stream, counter).await;
                        });
                    }
                }
            }
        });
        Ok(Self {
            addr,
            requests,
            shutdown: Some(tx),
        })
    }

    #[must_use]
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    #[must_use]
    pub fn requests(&self) -> u64 {
        self.requests.load(Ordering::SeqCst)
    }

    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

async fn handle(mut client: TcpStream, counter: Arc<AtomicU64>) -> std::io::Result<()> {
    let Some(header) = read_headers(&mut client).await? else {
        return Ok(());
    };
    let text = String::from_utf8_lossy(&header);
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let target = parts.next().unwrap_or("/");
    let version = parts.next().unwrap_or("HTTP/1.1");
    if method.eq_ignore_ascii_case("CONNECT") {
        client
            .write_all(b"HTTP/1.1 501 CONNECT is not supported by this fixture\r\n\r\n")
            .await?;
        return Ok(());
    }
    let uri: http::Uri = target.parse().map_err(std::io::Error::other)?;
    let host = uri
        .host()
        .ok_or_else(|| std::io::Error::other("missing host"))?;
    let port = uri.port_u16().unwrap_or(80);
    let path = uri
        .path_and_query()
        .map_or("/", http::uri::PathAndQuery::as_str);
    let mut upstream = TcpStream::connect((host, port)).await?;
    let mut request = format!("{method} {path} {version}\r\n");
    for line in lines {
        if line.is_empty() {
            break;
        }
        if line.to_ascii_lowercase().starts_with("proxy-connection:") {
            continue;
        }
        request.push_str(line);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    upstream.write_all(request.as_bytes()).await?;
    counter.fetch_add(1, Ordering::SeqCst);
    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
    Ok(())
}

async fn read_headers(stream: &mut TcpStream) -> std::io::Result<Option<Vec<u8>>> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(Some(buffer));
        }
        if buffer.len() > 64 * 1024 {
            return Err(std::io::Error::other("proxy header too large"));
        }
    }
}
