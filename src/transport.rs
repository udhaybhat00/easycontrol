use crate::protocol::{Frame, ProtocolError};
use bytes::BytesMut;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, timeout_at, Instant};

pub struct Transport<S = TcpStream> {
    stream: S,
    read_buf: BytesMut,
}

const MAX_READ_BUF: usize = 64 * 1024 * 1024 + 4; // Match the protocol frame limit, including length prefix
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

    /// Split ownership so a pending receive never blocks outgoing traffic.
    pub fn split(
        self,
    ) -> (
        Transport<tokio::net::tcp::OwnedReadHalf>,
        Transport<tokio::net::tcp::OwnedWriteHalf>,
    ) {
        let (reader, writer) = self.stream.into_split();
        (
            Transport {
                stream: reader,
                read_buf: self.read_buf,
            },
            Transport {
                stream: writer,
                read_buf: BytesMut::new(),
            },
        )
    }

    pub fn local_addr(&self) -> Result<std::net::SocketAddr, std::io::Error> {
        self.stream.local_addr()
    }
    pub fn peer_addr(&self) -> Result<std::net::SocketAddr, std::io::Error> {
        self.stream.peer_addr()
    }
}

impl<S: tokio::io::AsyncWrite + Unpin> Transport<S> {
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

    pub async fn shutdown(&mut self) -> Result<(), std::io::Error> {
        self.stream.shutdown().await
    }
}

impl<S: tokio::io::AsyncRead + Unpin> Transport<S> {
    pub async fn recv_frame(
        &mut self,
    ) -> Result<Option<Result<Frame, ProtocolError>>, std::io::Error> {
        // A partial-frame sender must not renew its deadline byte by byte.
        let deadline = Instant::now() + Duration::from_secs(READ_DEADLINE_SECS);
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
            let n = timeout_at(deadline, self.stream.read_buf(&mut self.read_buf)).await??;
            if n == 0 {
                if !self.read_buf.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "peer closed in the middle of a frame",
                    ));
                }
                return Ok(None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn truncated_frame_is_an_error_instead_of_clean_eof() {
        let (mut writer, reader) = tokio::io::duplex(64);
        writer.write_all(&[0, 0, 0, 12, 1, 2]).await.unwrap();
        drop(writer);
        let mut transport = Transport {
            stream: reader,
            read_buf: BytesMut::new(),
        };
        let error = transport.recv_frame().await.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test(start_paused = true)]
    async fn drip_fed_bytes_do_not_reset_the_frame_deadline() {
        let (mut writer, reader) = tokio::io::duplex(64);
        let slow_peer = tokio::spawn(async move {
            for byte in [0, 0, 0, 12] {
                tokio::time::sleep(Duration::from_secs(20)).await;
                if writer.write_all(&[byte]).await.is_err() {
                    break;
                }
            }
        });
        let mut transport = Transport {
            stream: reader,
            read_buf: BytesMut::new(),
        };
        let started = tokio::time::Instant::now();
        let error = transport.recv_frame().await.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert_eq!(started.elapsed(), Duration::from_secs(READ_DEADLINE_SECS));
        slow_peer.abort();
    }
}
