//! A tiny FTP server fixture for tests. It implements only the commands the
//! client needs: USER, PASS, TYPE, SIZE, PASV, REST, RETR, QUIT.
#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

#[derive(Debug, Clone)]
pub struct FtpFixtureConfig {
    pub file_name: String,
    pub content: Vec<u8>,
    pub user: String,
    pub password: String,
}

impl FtpFixtureConfig {
    #[must_use]
    pub fn new(content: Vec<u8>) -> Self {
        Self {
            file_name: "file.bin".into(),
            content,
            user: "user".into(),
            password: "pass".into(),
        }
    }
}

pub struct FtpTestServer {
    pub addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
}

impl FtpTestServer {
    pub async fn start(config: FtpFixtureConfig) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let config = Arc::new(config);
        let (tx, mut rx) = oneshot::channel();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut rx => break,
                    accepted = listener.accept() => {
                        let Ok((stream, _peer)) = accepted else { break };
                        let config = Arc::clone(&config);
                        tokio::spawn(async move {
                            let _ = handle_client(stream, config).await;
                        });
                    }
                }
            }
        });
        Ok(Self {
            addr,
            shutdown: Some(tx),
        })
    }

    #[must_use]
    pub fn url(&self) -> String {
        format!("ftp://user:pass@{}/file.bin", self.addr)
    }

    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

async fn handle_client(stream: TcpStream, config: Arc<FtpFixtureConfig>) -> std::io::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    write.write_all(b"220 RustGet FTP fixture\r\n").await?;
    let mut data_listener: Option<TcpListener> = None;
    let mut offset = 0_usize;
    let mut authenticated = false;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            break;
        }
        let command = line.trim_end_matches(['\r', '\n']).to_string();
        let (verb, argument) = command
            .split_once(' ')
            .map_or((command.as_str(), ""), |(verb, arg)| (verb, arg));
        match verb.to_ascii_uppercase().as_str() {
            "USER" => {
                authenticated = argument == config.user;
                write.write_all(b"331 Password required\r\n").await?;
            }
            "PASS" => {
                if authenticated && argument == config.password {
                    write.write_all(b"230 Logged in\r\n").await?;
                } else {
                    write.write_all(b"530 Login incorrect\r\n").await?;
                }
            }
            "TYPE" | "OPTS" | "NOOP" | "CWD" => {
                write.write_all(b"200 OK\r\n").await?;
            }
            "PWD" => write.write_all(b"257 \"/\" is current\r\n").await?,
            "FEAT" => {
                write
                    .write_all(b"211-Features:\r\n SIZE\r\n REST STREAM\r\n211 End\r\n")
                    .await?;
            }
            "SIZE" => {
                if argument.trim_start_matches('/') != config.file_name {
                    write.write_all(b"550 Not found\r\n").await?;
                } else {
                    write
                        .write_all(format!("213 {}\r\n", config.content.len()).as_bytes())
                        .await?;
                }
            }
            "REST" => {
                offset = argument.parse().unwrap_or(0);
                write
                    .write_all(format!("350 Restarting at {offset}\r\n").as_bytes())
                    .await?;
            }
            "PASV" => {
                let listener = TcpListener::bind("127.0.0.1:0").await?;
                let addr = listener.local_addr()?;
                data_listener = Some(listener);
                let port = addr.port();
                write
                    .write_all(
                        format!(
                            "227 Entering Passive Mode (127,0,0,1,{},{})\r\n",
                            port / 256,
                            port % 256
                        )
                        .as_bytes(),
                    )
                    .await?;
            }
            "RETR" => {
                if !authenticated {
                    write.write_all(b"530 Not logged in\r\n").await?;
                    continue;
                }
                if argument.trim_start_matches('/') != config.file_name {
                    write.write_all(b"550 Not found\r\n").await?;
                    continue;
                }
                let Some(listener) = data_listener.take() else {
                    write.write_all(b"425 Use PASV first\r\n").await?;
                    continue;
                };
                write.write_all(b"150 Opening data connection\r\n").await?;
                if let Ok((mut data, _)) = listener.accept().await {
                    let start = offset.min(config.content.len());
                    data.write_all(&config.content[start..]).await?;
                    let _ = data.shutdown().await;
                }
                write.write_all(b"226 Transfer complete\r\n").await?;
            }
            "QUIT" => {
                write.write_all(b"221 Bye\r\n").await?;
                break;
            }
            _ => {
                write.write_all(b"502 Not implemented\r\n").await?;
            }
        }
        write.flush().await?;
    }
    Ok(())
}
