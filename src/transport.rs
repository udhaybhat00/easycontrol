use bytes::BytesMut;
use crate::protocol::{Frame, ProtocolError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct Transport {
    stream: TcpStream,
    read_buf: BytesMut,
}

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
        let encoded = frame.encode().map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.stream.write_all(&encoded[..]).await?;
        Ok(())
    }

    pub async fn recv_frame(&mut self) -> Result<Option<Result<Frame, ProtocolError>>, std::io::Error> {
        loop {
            if let Some(result) = Frame::decode(&mut self.read_buf) {
                return Ok(Some(result));
            }
            let n = self.stream.read_buf(&mut self.read_buf).await?;
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
