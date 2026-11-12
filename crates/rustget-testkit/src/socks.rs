//! Minimal SOCKS5 CONNECT fixture for proxy tests.
#![forbid(unsafe_code)]

use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub struct SocksServer {
    addr: SocketAddr,
}

impl SocksServer {
    pub async fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        tokio::spawn(async move {
            loop {
                let Ok((client, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(handle(client));
            }
        });
        Ok(Self { addr })
    }

    #[must_use]
    pub fn url(&self) -> String {
        format!("socks5h://{}", self.addr)
    }
}

async fn handle(mut client: TcpStream) {
    let mut header = [0_u8; 2];
    if client.read_exact(&mut header).await.is_err() || header[0] != 5 {
        return;
    }
    let mut methods = vec![0_u8; header[1] as usize];
    if client.read_exact(&mut methods).await.is_err() {
        return;
    }
    if client.write_all(&[5, 0]).await.is_err() {
        return;
    }
    let mut request = [0_u8; 4];
    if client.read_exact(&mut request).await.is_err() || request[1] != 1 {
        return;
    }
    let host = match request[3] {
        1 => {
            let mut octets = [0_u8; 4];
            if client.read_exact(&mut octets).await.is_err() {
                return;
            }
            std::net::Ipv4Addr::from(octets).to_string()
        }
        3 => {
            let mut length = [0_u8; 1];
            if client.read_exact(&mut length).await.is_err() {
                return;
            }
            let mut name = vec![0_u8; length[0] as usize];
            if client.read_exact(&mut name).await.is_err() {
                return;
            }
            String::from_utf8_lossy(&name).to_string()
        }
        4 => {
            let mut octets = [0_u8; 16];
            if client.read_exact(&mut octets).await.is_err() {
                return;
            }
            std::net::Ipv6Addr::from(octets).to_string()
        }
        _ => return,
    };
    let mut port = [0_u8; 2];
    if client.read_exact(&mut port).await.is_err() {
        return;
    }
    let Ok(mut upstream) = TcpStream::connect((host.as_str(), u16::from_be_bytes(port))).await
    else {
        let _ = client.write_all(&[5, 1, 0, 1, 0, 0, 0, 0, 0, 0]).await;
        return;
    };
    if client
        .write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0])
        .await
        .is_err()
    {
        return;
    }
    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
}
