use crate::protocol::{Frame, ProtocolError};
use bytes::BytesMut;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

pub struct Transport {
    stream: TcpStream,
    read_buf: BytesMut,
}

const MAX_READ_BUF: usize = 4 * 1024 * 1024; // 4 MiB max buffered read data
const READ_DEADLINE_SECS: u64 = 30;

impl Transport {
    pub async fn connect(addr: &str) -> Result<Self, std::io::Error> {
        let stream = TcpStream::connect(addr).await?;
        Ok(Self::new(stream))
    }

    pub async fn listen(addr: &str) -> Result<tokio::net::TcpListener, std::io::Error> {
        tokio::net::TcpListener::bind(addr).await
    }

    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            read_buf: BytesMut::with_capacity(64 * 1024),
        }
    }

    pub async fn send_frame(&mut self, frame: &Frame) -> Result<(), std::io::Error> {
        let encoded = frame
            .encode()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        timeout(
            Duration::from_secs(READ_DEADLINE_SECS),
            self.stream.write_all(&encoded[..]),
        )
        .await??;
        Ok(())
    }

    pub async fn recv_frame(
        &mut self,
    ) -> Result<Option<Result<Frame, ProtocolError>>, std::io::Error> {
        loop {
            if let Some(result) = Frame::decode(&mut self.read_buf) {
                return Ok(Some(result));
            }
            // Enforce bounded read buffer to prevent memory exhaustion
            if self.read_buf.len() > MAX_READ_BUF {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "read buffer exceeded maximum size",
                ));
            }
            let n = timeout(
                Duration::from_secs(READ_DEADLINE_SECS),
                self.stream.read_buf(&mut self.read_buf),
            )
            .await??;
            if n == 0 {
                return Ok(None);
            }
        }
    }

    pub async fn shutdown(&mut self) -> Result<(), std::io::Error> {
        self.stream.shutdown().await
    }

    pub fn local_addr(&self) -> Result<std::net::SocketAddr, std::io::Error> {
        self.stream.local_addr()
    }

    pub fn peer_addr(&self) -> Result<std::net::SocketAddr, std::io::Error> {
        self.stream.peer_addr()
    }
}
