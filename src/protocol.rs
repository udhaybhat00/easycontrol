use bytes::{Buf, BufMut, BytesMut};
use serde::{Deserialize, Serialize};
use std::io::{Read as _, Write as _};
use thiserror::Error;

const MAX_FRAME_SIZE: usize = 64 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] bincode::Error),
    #[error("unknown channel {0}")]
    UnknownChannel(u8),
    #[error("frame too large")]
    FrameTooLarge,
    #[error("compression error: {0}")]
    Compression(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Channel {
    Control = 0,
    Screen = 1,
    Input = 2,
    File = 3,
    Clipboard = 4,
    Chat = 5,
}

impl Channel {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Result<Self, ProtocolError> {
        match v {
            0 => Ok(Channel::Control),
            1 => Ok(Channel::Screen),
            2 => Ok(Channel::Input),
            3 => Ok(Channel::File),
            4 => Ok(Channel::Clipboard),
            5 => Ok(Channel::Chat),
            _ => Err(ProtocolError::UnknownChannel(v)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlMsg {
    Hello { version: u32, session_id: String, public_key: [u8; 32] },
    HelloAck { accepted: bool, fingerprint: String, public_key: [u8; 32] },
    SetName { name: String },
    Ping,
    Pong,
    Bye { reason: String },
    Permissions { input: bool, file: bool, clipboard: bool, chat: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScreenMsg {
    FrameInit { width: u32, height: u32, stride: u32, format: String },
    FrameData { seq: u64, data: Vec<u8>, compressed: bool },
    FrameAck { seq: u64 },
    Cursor { x: i32, y: i32 },
    MonitorList { monitors: Vec<MonitorInfo> },
    MonitorSelect { index: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub index: usize,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InputMsg {
    MouseMove { x: i32, y: i32 },
    MousePress { button: u8, x: i32, y: i32 },
    MouseRelease { button: u8, x: i32, y: i32 },
    MouseScroll { dx: i32, dy: i32 },
    KeyPress { code: u16 },
    KeyRelease { code: u16 },
    KeyType { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileMsg {
    ListDir { path: String },
    ListDirAck { entries: Vec<FileEntry> },
    ReadRequest { path: String, offset: u64, length: u32 },
    ReadData { path: String, offset: u64, data: Vec<u8> },
    WriteRequest { path: String, offset: u64, data: Vec<u8> },
    WriteAck { path: String, offset: u64, bytes_written: u32 },
    Cancel { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClipboardMsg {
    Set { data: Vec<u8>, mime: String },
    Get,
    GetAck { data: Vec<u8>, mime: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatMsg {
    Message { from: String, text: String, ts: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub channel: Channel,
    pub seq: u64,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn encode(&self) -> Result<BytesMut, ProtocolError> {
        let payload = bincode::serialize(self)?;
        if payload.len() > MAX_FRAME_SIZE {
            return Err(ProtocolError::FrameTooLarge);
        }
        let mut buf = BytesMut::with_capacity(4 + payload.len());
        buf.put_u32(payload.len() as u32);
        buf.extend_from_slice(&payload);
        Ok(buf)
    }

    pub fn decode(buf: &mut BytesMut) -> Option<Result<Frame, ProtocolError>> {
        if buf.len() < 4 {
            return None;
        }
        let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        if len > MAX_FRAME_SIZE {
            buf.clear();
            return Some(Err(ProtocolError::FrameTooLarge));
        }
        if buf.len() < 4 + len {
            return None;
        }
        buf.advance(4);
        let payload = buf.split_to(len);
        match bincode::deserialize::<Frame>(payload.as_ref()) {
            Ok(f) => Some(Ok(f)),
            Err(e) => Some(Err(ProtocolError::Serialization(e))),
        }
    }
}

#[cfg(feature = "compression")]
pub fn compress_payload(data: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 3)
        .map_err(|e| ProtocolError::Compression(e.to_string()))?;
    encoder
        .write_all(data)
        .map_err(|e| ProtocolError::Compression(e.to_string()))?;
    encoder
        .finish()
        .map_err(|e| ProtocolError::Compression(e.to_string()))
}

#[cfg(feature = "compression")]
pub fn decompress_payload(data: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    let mut decoder = zstd::stream::read::Decoder::new(data)
        .map_err(|e| ProtocolError::Compression(e.to_string()))?;
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut decoder, &mut out)
        .map_err(|e| ProtocolError::Compression(e.to_string()))?;
    Ok(out)
}

#[cfg(not(feature = "compression"))]
pub fn compress_payload(data: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    Ok(data.to_vec())
}

#[cfg(not(feature = "compression"))]
pub fn decompress_payload(data: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    Ok(data.to_vec())
}
