//! Core application logic for easycontrol.
use crate::clipboard::{validate_bytes, ClipboardBackend, SystemClipboard};
use crate::crypto::{derive_session_key, SessionCipher};
use crate::file_transfer::FileSession;
use crate::input::InputSession;
use crate::protocol::{
    Channel, ChatMsg, ClipboardMsg, ControlMsg, FileMsg, Frame, InputMsg, ScreenMsg,
    PROTOCOL_VERSION,
};
use crate::relay::{connect_as_client, connect_as_host, RelayServer};
use crate::screen::ScreenSession;
use crate::transport::Transport;
use base64::Engine as _;
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StateMutex};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, watch, Mutex};
use tokio::task::JoinSet;
use tracing::info;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const IDLE_TIMEOUT: Duration = Duration::from_secs(45);

/// Configuration for starting an easycontrol session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub listen_addr: Option<SocketAddr>,
    pub connect_addr: Option<SocketAddr>,
    pub relay_addr: Option<SocketAddr>,
    pub session_id: Option<String>,
    pub password: Option<String>,
    pub file_root: Option<PathBuf>,
    pub enable_input: bool,
    /// Allow network-only sessions without invoking an OS capture backend.
    pub enable_capture: bool,
    pub enable_clipboard: bool,
    pub enable_chat: bool,
    pub enable_file_transfer: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            listen_addr: None,
            connect_addr: None,
            relay_addr: None,
            session_id: None,
            password: None,
            file_root: Some("./easycontrol_files".into()),
            enable_input: false,
            enable_capture: true,
            enable_clipboard: false,
            enable_chat: false,
            enable_file_transfer: false,
        }
    }
}

/// Events emitted by a session.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    Connected {
        session_id: String,
    },
    Disconnected {
        reason: String,
    },
    ScreenFrame {
        width: u32,
        height: u32,
        stride: u32,
        format: String,
        data: Vec<u8>,
    },
    /// Local host capture health; this is not a wire-protocol change.
    ScreenSharing {
        active: bool,
    },
    MonitorList {
        monitors: Vec<MonitorInfo>,
    },
    ClipboardData {
        data: Vec<u8>,
        mime: String,
    },
    FileList {
        entries: Vec<crate::protocol::FileEntry>,
    },
    FileData {
        path: String,
        offset: u64,
        data: Vec<u8>,
    },
    FileWritten {
        path: String,
        offset: u64,
        bytes_written: u32,
    },
    PermissionDenied {
        channel: Channel,
        reason: String,
    },
    ChatMessage {
        from: String,
        text: String,
    },
    Error {
        message: String,
    },
}

/// Monitor information for multi-monitor support.
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub index: usize,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

/// Commands sent to a session.
#[derive(Debug)]
pub enum SessionCommand {
    MouseMove {
        x: i32,
        y: i32,
    },
    MouseClick {
        button: u8,
    },
    MousePress {
        button: u8,
        x: i32,
        y: i32,
    },
    MouseRelease {
        button: u8,
        x: i32,
        y: i32,
    },
    MouseScroll {
        dx: i32,
        dy: i32,
    },
    KeyPress {
        code: u16,
    },
    KeyRelease {
        code: u16,
    },
    KeyType {
        text: String,
    },
    ClipboardGet,
    ClipboardSet {
        text: String,
    },
    FileList {
        path: String,
    },
    FileRead {
        path: String,
        offset: u64,
        length: u32,
    },
    FileWrite {
        path: String,
        offset: u64,
        data: Vec<u8>,
    },
    ChatMessage {
        text: String,
    },
    MonitorSelect {
        index: usize,
    },
    Disconnect,
}

/// Session lifecycle states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Connecting,
    Authenticating,
    Connected,
    Disconnecting,
    Disconnected,
}

/// Core error type.
#[derive(Debug)]
pub enum CoreError {
    Io(std::io::Error),
    Serialization(String),
    Protocol(String),
    Crypto(String),
    Handshake(String),
    Session(String),
    IoTimeout(Duration),
}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        CoreError::Io(e)
    }
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoreError::Io(e) => write!(f, "IO error: {}", e),
            CoreError::Serialization(s) => write!(f, "Serialization error: {}", s),
            CoreError::Protocol(s) => write!(f, "Protocol error: {}", s),
            CoreError::Crypto(s) => write!(f, "Crypto error: {}", s),
            CoreError::Handshake(s) => write!(f, "Handshake error: {}", s),
            CoreError::Session(s) => write!(f, "Session error: {}", s),
            CoreError::IoTimeout(d) => write!(f, "I/O timeout after {:?}", d),
        }
    }
}

impl std::error::Error for CoreError {}

/// A cloneable session handle. Dropping the last handle cancels all workers.
#[derive(Clone)]
pub struct SessionHandle {
    inner: Arc<SessionInner>,
}

/// Event subscription with an initial lifecycle snapshot and bounded live events.
pub struct SessionEvents {
    initial: Option<SessionEvent>,
    receiver: broadcast::Receiver<SessionEvent>,
}
impl SessionEvents {
    pub async fn recv(&mut self) -> Result<SessionEvent, broadcast::error::RecvError> {
        if let Some(event) = self.initial.take() {
            return Ok(event);
        }
        self.receiver.recv().await
    }
}

pub struct SessionInner {
    id: String,
    state: Arc<StateMutex<SessionState>>,
    command_tx: mpsc::Sender<SessionCommand>,
    event_tx: broadcast::Sender<SessionEvent>,
    shutdown_tx: watch::Sender<bool>,
    done: watch::Receiver<bool>,
    is_host: bool,
    input_allowed: Arc<AtomicBool>,
    input: Arc<Mutex<Option<InputSession>>>,
}

impl Drop for SessionInner {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
    }
}

impl SessionHandle {
    pub fn id(&self) -> &str {
        &self.inner.id
    }
    pub fn state(&self) -> SessionState {
        self.inner.state.lock().unwrap().clone()
    }
    /// Bounded broadcast: slow consumers must handle RecvError::Lagged.
    pub fn event_stream(&self) -> SessionEvents {
        let receiver = self.inner.event_tx.subscribe();
        let initial = if self.state() == SessionState::Connected {
            SessionEvent::Connected {
                session_id: self.id().to_string(),
            }
        } else {
            SessionEvent::Disconnected {
                reason: "session closed".into(),
            }
        };
        SessionEvents {
            initial: Some(initial),
            receiver,
        }
    }
    pub async fn send_command(&self, cmd: SessionCommand) -> Result<(), CoreError> {
        if matches!(cmd, SessionCommand::Disconnect) {
            return self.disconnect().await;
        }
        if self.state() != SessionState::Connected {
            return Err(CoreError::Session("session closed".into()));
        }
        self.inner
            .command_tx
            .send(cmd)
            .await
            .map_err(|_| CoreError::Session("session closed".into()))
    }
    /// Host-only, immediate input policy control. Enabling requires that the
    /// local OS input backend can be initialized; clients cannot call this.
    pub async fn set_remote_input_enabled(&self, enabled: bool) -> Result<(), CoreError> {
        if !self.inner.is_host {
            return Err(CoreError::Session(
                "only the host can approve remote input".into(),
            ));
        }
        let mut input = self.inner.input.lock().await;
        if enabled && input.is_none() {
            *input = Some(
                InputSession::try_default()
                    .map_err(|error| CoreError::Session(error.to_string()))?,
            );
        }
        if let Some(input) = input.as_mut() {
            input.set_enabled(enabled);
        }
        self.inner.input_allowed.store(enabled, Ordering::Release);
        Ok(())
    }
    pub fn remote_input_enabled(&self) -> bool {
        self.inner.input_allowed.load(Ordering::Acquire)
    }
    /// Wait until workers have exited and the socket halves have been dropped.
    pub async fn disconnect(&self) -> Result<(), CoreError> {
        if self.state() == SessionState::Connected {
            *self.inner.state.lock().unwrap() = SessionState::Disconnecting;
        }
        let _ = self.inner.shutdown_tx.send(true);
        let mut done = self.inner.done.clone();
        while !*done.borrow_and_update() {
            if done.changed().await.is_err() {
                break;
            }
        }
        Ok(())
    }
}

pub struct SessionManager;

impl SessionManager {
    pub async fn start_host(mut config: SessionConfig) -> Result<SessionHandle, CoreError> {
        validate_password(config.password.as_deref())?;
        let stream = if let Some(relay) = config.relay_addr {
            let id = config.session_id.clone().unwrap_or_else(random_session_id);
            info!("registering relay session {}", id);
            config.session_id = Some(id.clone());
            connect_as_host(
                &relay.to_string(),
                &id,
                config.password.as_deref().expect("validated password"),
            )
            .await?
        } else {
            let addr = config
                .listen_addr
                .unwrap_or_else(|| "0.0.0.0:5899".parse().unwrap());
            let listener = TcpListener::bind(addr).await?;
            info!("easycontrol host listening on {}", listener.local_addr()?);
            return Self::start_host_listener(listener, config).await;
        };
        Self::handle_peer(stream, config, true, None, None).await
    }

    /// Accept on an already-bound listener so desktop callers can report readiness
    /// only after binding succeeds. Failed authentication never consumes the host.
    pub async fn start_host_listener(
        listener: TcpListener,
        config: SessionConfig,
    ) -> Result<SessionHandle, CoreError> {
        validate_password(config.password.as_deref())?;
        loop {
            let (stream, _) = listener.accept().await?;
            match Self::handle_peer(stream, config.clone(), true, None, None).await {
                Ok(handle) => return Ok(handle),
                Err(CoreError::Handshake(_) | CoreError::IoTimeout(_)) => continue,
                Err(error) => return Err(error),
            }
        }
    }
    pub async fn start_client(config: SessionConfig) -> Result<SessionHandle, CoreError> {
        validate_password(config.password.as_deref())?;
        let stream = if let Some(relay) = config.relay_addr {
            let id = config
                .session_id
                .as_deref()
                .ok_or_else(|| CoreError::Session("session_id required for relay mode".into()))?;
            connect_as_client(
                &relay.to_string(),
                id,
                config.password.as_deref().expect("validated password"),
            )
            .await?
        } else {
            let addr = config.connect_addr.ok_or_else(|| {
                CoreError::Session("connect_addr required for client mode".into())
            })?;
            tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(addr))
                .await
                .map_err(|_| CoreError::IoTimeout(Duration::from_secs(10)))??
        };
        Self::handle_peer(stream, config, false, None, None).await
    }
    pub async fn start_relay(listen_addr: SocketAddr) -> Result<RelayHandle, CoreError> {
        let listener = TcpListener::bind(listen_addr).await?;
        let addr = listener.local_addr()?;
        let server = RelayServer::new(addr);
        let handle = tokio::spawn(async move {
            let _ = server.run_listener(listener).await;
        });
        Ok(RelayHandle {
            id: handle.id().to_string(),
            addr,
            handle,
        })
    }

    // Tests can supply deterministic capture/input backends without operating the desktop.
    async fn handle_peer(
        stream: TcpStream,
        config: SessionConfig,
        is_host: bool,
        screen: Option<ScreenSession>,
        input: Option<InputSession>,
    ) -> Result<SessionHandle, CoreError> {
        validate_password(config.password.as_deref())?;
        let mut transport = Transport::new(stream);
        let authentication = async {
            let (id, mut cipher) = if is_host {
                host_spake2_handshake(
                    &mut transport,
                    config.password.as_deref(),
                    config.session_id.as_deref(),
                )
                .await?
            } else {
                client_spake2_handshake(
                    &mut transport,
                    config.session_id.as_deref(),
                    config.password.as_deref(),
                )
                .await?
            };
            confirm_session(&mut transport, &mut cipher, is_host).await?;
            anyhow::Ok((id, cipher))
        };
        let (id, cipher) = tokio::time::timeout(Duration::from_secs(10), authentication)
            .await
            .map_err(|_| CoreError::IoTimeout(Duration::from_secs(10)))?
            .map_err(|e| CoreError::Handshake(e.to_string()))?;
        let screen = is_host.then(|| {
            let screen = screen.unwrap_or_else(|| {
                if config.enable_capture {
                    ScreenSession::default()
                } else {
                    ScreenSession::new(Box::new(crate::screen::NullCapture))
                        .expect("null monitor exists")
                }
            });
            Arc::new(Mutex::new(screen))
        });
        // Input is host-owned. A disabled host can initialize its backend only
        // when the local user later approves remote control.
        let input = if is_host && config.enable_input {
            let mut input = match input {
                Some(input) => input,
                None => {
                    InputSession::try_default().map_err(|e| CoreError::Session(e.to_string()))?
                }
            };
            input.set_enabled(true);
            Some(input)
        } else {
            None
        };
        let input = Arc::new(Mutex::new(input));
        let input_allowed = Arc::new(AtomicBool::new(is_host && config.enable_input));
        let files = if is_host && config.enable_file_transfer {
            let root = config.file_root.clone().ok_or_else(|| {
                CoreError::Session("file_root required when file transfer is enabled".into())
            })?;
            Some(Arc::new(
                FileSession::new(root)
                    .await
                    .map_err(|error| CoreError::Session(error.to_string()))?,
            ))
        } else {
            None
        };
        let clipboard: Option<Box<dyn ClipboardBackend>> = if is_host && config.enable_clipboard {
            Some(Box::new(SystemClipboard))
        } else {
            None
        };
        let (reader, writer) = transport.split();
        let writer = Arc::new(Mutex::new(writer));
        let cipher = Arc::new(Mutex::new(cipher));
        let seq = Arc::new(AtomicU64::new(2)); // confirmation consumed sequence 1
        let (command_tx, mut command_rx) = mpsc::channel::<SessionCommand>(64);
        let (event_tx, _) = broadcast::channel(32);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (done_tx, done) = watch::channel(false);
        let state = Arc::new(StateMutex::new(SessionState::Connected));
        let handle = SessionHandle {
            inner: Arc::new(SessionInner {
                id: id.clone(),
                state: state.clone(),
                command_tx,
                event_tx: event_tx.clone(),
                shutdown_tx,
                done,
                is_host,
                input_allowed: input_allowed.clone(),
                input: input.clone(),
            }),
        };
        let mut tasks = JoinSet::new();
        let cleanup_input = input.clone();
        let send_writer = writer.clone();
        let send_cipher = cipher.clone();
        let send_seq = seq.clone();
        tasks.spawn(async move {
            while let Some(command) = command_rx.recv().await {
                // Serialize sequence assignment and writing to preserve wire order.
                let mut writer = send_writer.lock().await;
                let n = send_seq.fetch_add(1, Ordering::Relaxed);
                let frame = cmd_to_frame(&mut *send_cipher.lock().await, n, &command)?;
                writer.send_frame(&frame).await?;
            }
            anyhow::Ok(())
        });
        let heartbeat_writer = writer.clone();
        let heartbeat_cipher = cipher.clone();
        let heartbeat_seq = seq.clone();
        tasks.spawn(async move {
            let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
            loop {
                interval.tick().await;
                send_channel_msg(
                    &heartbeat_writer,
                    &heartbeat_cipher,
                    &heartbeat_seq,
                    Channel::Control,
                    &ControlMsg::Ping,
                )
                .await?;
            }
            #[allow(unreachable_code)]
            anyhow::Ok(())
        });
        let recv_writer = writer.clone();
        let recv_cipher = cipher.clone();
        let recv_seq = seq.clone();
        let recv_screen = screen.clone();
        let recv_events = event_tx.clone();
        let recv_files = files.clone();
        tasks.spawn(async move {
            receive_loop(
                reader,
                recv_writer,
                recv_cipher,
                recv_seq,
                recv_screen,
                input,
                input_allowed,
                recv_files,
                clipboard,
                recv_events,
                config,
                is_host,
            )
            .await
        });
        if let Some(screen) = screen {
            let capture_events = event_tx.clone();
            let capture_writer = writer.clone();
            let capture_cipher = cipher.clone();
            let capture_seq = seq.clone();
            tasks.spawn(async move {
                let monitors = screen.lock().await.list_monitors()?;
                send_channel_msg(
                    &capture_writer,
                    &capture_cipher,
                    &capture_seq,
                    Channel::Screen,
                    &ScreenMsg::MonitorList { monitors },
                )
                .await?;
                let mut interval = tokio::time::interval(Duration::from_millis(100));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let mut inventory_tick = 0u32;
                let mut sharing = false;
                let mut last_capture_error = None;
                loop {
                    interval.tick().await;
                    inventory_tick += 1;
                    if inventory_tick.is_multiple_of(20) {
                        let monitors = screen.lock().await.list_monitors()?;
                        send_channel_msg(
                            &capture_writer,
                            &capture_cipher,
                            &capture_seq,
                            Channel::Screen,
                            &ScreenMsg::MonitorList { monitors },
                        )
                        .await?;
                    }
                    let captured = {
                        let mut screen = screen.lock().await;
                        screen.capture_frame().and_then(|data| {
                            screen
                                .current_dimensions()
                                .map(|dimensions| (data, dimensions))
                        })
                    };
                    match captured {
                        Ok((data, (width, height))) => {
                            last_capture_error = None;
                            send_channel_msg(
                                &capture_writer,
                                &capture_cipher,
                                &capture_seq,
                                Channel::Screen,
                                &ScreenMsg::FrameInit {
                                    width,
                                    height,
                                    stride: width * 4,
                                    format: "RGBA8".into(),
                                },
                            )
                            .await?;
                            // Retina frames can be tens of megabytes. Use the existing
                            // protocol compression flag when it reduces the payload so a
                            // bounded viewer can receive a frame before the next capture.
                            let compressed_data = crate::protocol::compress_payload(&data)?;
                            let (data, compressed) = if compressed_data.len() < data.len() {
                                (compressed_data, true)
                            } else {
                                (data, false)
                            };
                            send_channel_msg(
                                &capture_writer,
                                &capture_cipher,
                                &capture_seq,
                                Channel::Screen,
                                &ScreenMsg::FrameData {
                                    seq: 0,
                                    data,
                                    compressed,
                                },
                            )
                            .await?;
                            if !sharing || inventory_tick.is_multiple_of(20) {
                                let _ = capture_events
                                    .send(SessionEvent::ScreenSharing { active: true });
                                sharing = true;
                            }
                        }
                        Err(crate::screen::ScreenError::NotSupported) => {
                            // Keep the session alive when built without capture.
                            std::future::pending::<()>().await;
                        }
                        Err(e) => {
                            if sharing {
                                let _ = capture_events
                                    .send(SessionEvent::ScreenSharing { active: false });
                                sharing = false;
                            }
                            let message = e.to_string();
                            if last_capture_error.as_ref() != Some(&message) {
                                let _ = capture_events.send(SessionEvent::Error {
                                    message: message.clone(),
                                });
                                last_capture_error = Some(message);
                            }
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                    }
                }
                #[allow(unreachable_code)]
                anyhow::Ok(())
            });
        }
        // Supervisor owns every worker; no worker owns the SessionHandle.
        tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;
            let reason = tokio::select! {
                _ = shutdown_rx.changed() => "local disconnect".to_string(),
                result = tasks.join_next() => match result {
                    Some(Ok(Err(e))) => e.to_string(),
                    Some(Err(e)) => format!("session worker failed: {}", e),
                    _ => "peer disconnected".into(),
                },
            };
            *state.lock().unwrap() = SessionState::Disconnecting;
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            // Best effort goodbye, bounded so disconnect cannot hang behind backpressure.
            let bye = ControlMsg::Bye {
                reason: reason.clone(),
            };
            let _ = tokio::time::timeout(
                Duration::from_millis(250),
                send_channel_msg(&writer, &cipher, &seq, Channel::Control, &bye),
            )
            .await;
            let _ = tokio::time::timeout(Duration::from_millis(250), async {
                writer.lock().await.shutdown().await
            })
            .await;
            drop(writer);
            // A handle may outlive the workers. Release the OS-backed input
            // backend here so disconnect does not retain an injector or its
            // resources until the last handle is dropped.
            *cleanup_input.lock().await = None;
            *state.lock().unwrap() = SessionState::Disconnected;
            let _ = event_tx.send(SessionEvent::Disconnected { reason });
            let _ = done_tx.send(true);
        });
        Ok(handle)
    }
}

pub struct RelayHandle {
    id: String,
    addr: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
}
impl RelayHandle {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }
    pub async fn stop(&self) {
        self.handle.abort();
        while !self.handle.is_finished() {
            tokio::task::yield_now().await;
        }
    }
}
impl Drop for RelayHandle {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

type Writer = Arc<Mutex<Transport<tokio::net::tcp::OwnedWriteHalf>>>;

#[allow(clippy::too_many_arguments)]
async fn receive_loop(
    mut reader: Transport<tokio::net::tcp::OwnedReadHalf>,
    writer: Writer,
    cipher: Arc<Mutex<SessionCipher>>,
    seq: Arc<AtomicU64>,
    screen: Option<Arc<Mutex<ScreenSession>>>,
    input: Arc<Mutex<Option<InputSession>>>,
    input_allowed: Arc<AtomicBool>,
    files: Option<Arc<FileSession>>,
    mut clipboard: Option<Box<dyn ClipboardBackend>>,
    events: broadcast::Sender<SessionEvent>,
    config: SessionConfig,
    is_host: bool,
) -> anyhow::Result<()> {
    let mut last_seq = 1;
    let mut dimensions = None;
    while let Some(frame) = tokio::time::timeout(IDLE_TIMEOUT, reader.recv_frame())
        .await
        .map_err(|_| anyhow::anyhow!("connection idle timeout"))??
    {
        let frame = frame?;
        let plain = decrypt_frame(&mut *cipher.lock().await, &frame, &mut last_seq)?;
        match frame.channel {
            Channel::Control => match bincode::deserialize::<ControlMsg>(&plain)? {
                ControlMsg::Bye { .. } => return Ok(()),
                ControlMsg::Ping => {
                    send_channel_msg(&writer, &cipher, &seq, Channel::Control, &ControlMsg::Pong)
                        .await?
                }
                ControlMsg::Denied { channel, reason } => {
                    let _ = events.send(SessionEvent::PermissionDenied { channel, reason });
                }
                _ => {}
            },
            Channel::Input if is_host && input_allowed.load(Ordering::Acquire) => {
                let msg: InputMsg = bincode::deserialize(&plain)?;
                if let Some(input) = input.lock().await.as_ref() {
                    if let Err(e) = input.inject(&msg) {
                        let _ = events.send(SessionEvent::Error {
                            message: e.to_string(),
                        });
                    }
                }
            }
            Channel::Input if is_host => {
                deny(
                    &writer,
                    &cipher,
                    &seq,
                    Channel::Input,
                    "remote input is disabled",
                )
                .await?
            }
            Channel::Screen => match bincode::deserialize::<ScreenMsg>(&plain)? {
                ScreenMsg::FrameInit {
                    width,
                    height,
                    stride,
                    format,
                } if !is_host => {
                    dimensions = Some((width, height, stride, format));
                }
                ScreenMsg::FrameData {
                    data, compressed, ..
                } if !is_host => {
                    let (width, height, stride, format) = dimensions
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("frame data before frame init"))?;
                    let data = if compressed {
                        crate::protocol::decompress_payload(&data)?
                    } else {
                        data
                    };
                    let _ = events.send(SessionEvent::ScreenFrame {
                        width,
                        height,
                        stride,
                        format,
                        data,
                    });
                }
                ScreenMsg::MonitorList { monitors } if !is_host => {
                    let monitors = monitors
                        .into_iter()
                        .map(|m| MonitorInfo {
                            index: m.index,
                            x: m.x,
                            y: m.y,
                            width: m.width,
                            height: m.height,
                            primary: m.primary,
                        })
                        .collect();
                    let _ = events.send(SessionEvent::MonitorList { monitors });
                }
                ScreenMsg::MonitorSelect { index } if is_host => {
                    if let Some(screen) = &screen {
                        let monitors = {
                            let mut screen = screen.lock().await;
                            screen.select_monitor(index)?;
                            screen.list_monitors()?
                        };
                        send_channel_msg(
                            &writer,
                            &cipher,
                            &seq,
                            Channel::Screen,
                            &ScreenMsg::MonitorList { monitors },
                        )
                        .await?;
                    }
                }
                _ => {}
            },
            Channel::Clipboard if is_host && config.enable_clipboard => {
                match bincode::deserialize::<ClipboardMsg>(&plain)? {
                    ClipboardMsg::Get => {
                        let (data, mime) = match clipboard
                            .as_mut()
                            .ok_or_else(|| anyhow::anyhow!("clipboard backend unavailable"))?
                            .get_text()
                        {
                            Ok(text) => (text.into_bytes(), "text/plain".to_string()),
                            Err(error) => {
                                let _ = events.send(SessionEvent::Error {
                                    message: error.to_string(),
                                });
                                (Vec::new(), "text/plain".to_string())
                            }
                        };
                        send_channel_msg(
                            &writer,
                            &cipher,
                            &seq,
                            Channel::Clipboard,
                            &ClipboardMsg::GetAck { data, mime },
                        )
                        .await?
                    }
                    ClipboardMsg::Set { data, mime } => {
                        let text = match validate_bytes(&data) {
                            Ok(text) => text,
                            Err(error) => {
                                deny(
                                    &writer,
                                    &cipher,
                                    &seq,
                                    Channel::Clipboard,
                                    &error.to_string(),
                                )
                                .await?;
                                continue;
                            }
                        };
                        if mime != "text/plain" {
                            deny(
                                &writer,
                                &cipher,
                                &seq,
                                Channel::Clipboard,
                                "only text/plain is supported",
                            )
                            .await?;
                            continue;
                        }
                        if let Err(error) = clipboard
                            .as_mut()
                            .ok_or_else(|| anyhow::anyhow!("clipboard backend unavailable"))?
                            .set_text(text)
                        {
                            let _ = events.send(SessionEvent::Error {
                                message: error.to_string(),
                            });
                        }
                        let _ = events.send(SessionEvent::ClipboardData { data, mime });
                    }
                    ClipboardMsg::GetAck { data, mime } => {
                        let _ = events.send(SessionEvent::ClipboardData { data, mime });
                    }
                }
            }
            Channel::Clipboard if is_host => {
                deny(
                    &writer,
                    &cipher,
                    &seq,
                    Channel::Clipboard,
                    "clipboard sharing is disabled",
                )
                .await?
            }
            Channel::Clipboard if !is_host => match bincode::deserialize::<ClipboardMsg>(&plain)? {
                ClipboardMsg::GetAck { data, mime } => {
                    let text = validate_bytes(&data)
                        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                    if mime != "text/plain" {
                        return Err(anyhow::anyhow!("unsupported clipboard MIME type"));
                    }
                    let _ = events.send(SessionEvent::ClipboardData {
                        data: text.as_bytes().to_vec(),
                        mime,
                    });
                }
                _ => return Err(anyhow::anyhow!("unexpected clipboard request from host")),
            },
            Channel::Chat if config.enable_chat => {
                let ChatMsg::Message { from, text, .. } = bincode::deserialize(&plain)?;
                let _ = events.send(SessionEvent::ChatMessage { from, text });
            }
            Channel::Chat if is_host => {
                deny(&writer, &cipher, &seq, Channel::Chat, "chat is disabled").await?
            }
            Channel::File if is_host && config.enable_file_transfer => {
                let file = files
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("file backend unavailable"))?;
                match bincode::deserialize::<FileMsg>(&plain)? {
                    FileMsg::ListDir { path } => match file.list_dir(&path).await {
                        Ok(entries) => {
                            send_channel_msg(
                                &writer,
                                &cipher,
                                &seq,
                                Channel::File,
                                &FileMsg::ListDirAck { entries },
                            )
                            .await?
                        }
                        Err(error) => {
                            deny(&writer, &cipher, &seq, Channel::File, &error.to_string()).await?
                        }
                    },
                    FileMsg::ReadRequest {
                        path,
                        offset,
                        length,
                    } => match file.read_chunk(&path, offset, length).await {
                        Ok(data) => {
                            send_channel_msg(
                                &writer,
                                &cipher,
                                &seq,
                                Channel::File,
                                &FileMsg::ReadData { path, offset, data },
                            )
                            .await?
                        }
                        Err(error) => {
                            deny(&writer, &cipher, &seq, Channel::File, &error.to_string()).await?
                        }
                    },
                    FileMsg::WriteRequest { path, offset, data } => {
                        match file.write_chunk(&path, offset, &data).await {
                            Ok(bytes_written) => {
                                send_channel_msg(
                                    &writer,
                                    &cipher,
                                    &seq,
                                    Channel::File,
                                    &FileMsg::WriteAck {
                                        path,
                                        offset,
                                        bytes_written,
                                    },
                                )
                                .await?
                            }
                            Err(error) => {
                                deny(&writer, &cipher, &seq, Channel::File, &error.to_string())
                                    .await?
                            }
                        }
                    }
                    _ => {
                        deny(
                            &writer,
                            &cipher,
                            &seq,
                            Channel::File,
                            "unexpected file response",
                        )
                        .await?
                    }
                }
            }
            Channel::File if is_host => {
                deny(
                    &writer,
                    &cipher,
                    &seq,
                    Channel::File,
                    "file transfer is disabled",
                )
                .await?
            }
            Channel::File if !is_host => match bincode::deserialize::<FileMsg>(&plain)? {
                FileMsg::ListDirAck { entries } => {
                    let _ = events.send(SessionEvent::FileList { entries });
                }
                FileMsg::ReadData { path, offset, data } => {
                    let _ = events.send(SessionEvent::FileData { path, offset, data });
                }
                FileMsg::WriteAck {
                    path,
                    offset,
                    bytes_written,
                } => {
                    let _ = events.send(SessionEvent::FileWritten {
                        path,
                        offset,
                        bytes_written,
                    });
                }
                _ => return Err(anyhow::anyhow!("unexpected file request from host")),
            },
            _ => {}
        }
    }
    Ok(())
}

fn random_session_id() -> String {
    let mut bytes = [0; 32];
    getrandom::getrandom(&mut bytes).expect("OS randomness unavailable");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn validate_password(password: Option<&str>) -> Result<(), CoreError> {
    match password {
        Some(value) if !value.trim().is_empty() => Ok(()),
        _ => Err(CoreError::Session(
            "a non-empty password is required for unattended access".into(),
        )),
    }
}

const CLIENT_IDENTITY: &[u8] = b"easycontrol-v4-client";
const SERVER_IDENTITY: &[u8] = b"easycontrol-v4-host";

// Authenticate the routing channel and sequence inside the encrypted payload.
fn encrypted_frame<T: serde::Serialize>(
    cipher: &mut SessionCipher,
    seq: u64,
    channel: Channel,
    msg: &T,
) -> anyhow::Result<Frame> {
    let mut plain = vec![channel.as_u8()];
    plain.extend_from_slice(&seq.to_le_bytes());
    plain.extend_from_slice(&bincode::serialize(msg)?);
    Ok(Frame {
        channel,
        seq,
        payload: cipher.encrypt(&plain),
    })
}
fn decrypt_frame(
    cipher: &mut SessionCipher,
    frame: &Frame,
    last_seq: &mut u64,
) -> anyhow::Result<Vec<u8>> {
    let plain = cipher.decrypt(&frame.payload)?;
    anyhow::ensure!(plain.len() >= 9, "missing authenticated header");
    let seq = u64::from_le_bytes(plain[1..9].try_into()?);
    anyhow::ensure!(
        plain[0] == frame.channel.as_u8() && seq == frame.seq,
        "authenticated header mismatch"
    );
    anyhow::ensure!(seq > *last_seq, "replayed or out-of-order frame");
    *last_seq = seq;
    Ok(plain[9..].to_vec())
}

async fn send_channel_msg<T: serde::Serialize>(
    writer: &Writer,
    cipher: &Arc<Mutex<SessionCipher>>,
    next_seq: &AtomicU64,
    channel: Channel,
    msg: &T,
) -> anyhow::Result<()> {
    let mut writer = writer.lock().await;
    let seq = next_seq.fetch_add(1, Ordering::Relaxed);
    let frame = encrypted_frame(&mut *cipher.lock().await, seq, channel, msg)?;
    writer.send_frame(&frame).await?;
    Ok(())
}

async fn deny(
    writer: &Writer,
    cipher: &Arc<Mutex<SessionCipher>>,
    next_seq: &AtomicU64,
    channel: Channel,
    reason: &str,
) -> anyhow::Result<()> {
    send_channel_msg(
        writer,
        cipher,
        next_seq,
        Channel::Control,
        &ControlMsg::Denied {
            channel,
            reason: reason.into(),
        },
    )
    .await
}

async fn read_handshake(transport: &mut Transport) -> anyhow::Result<Frame> {
    let frame = transport
        .recv_frame()
        .await?
        .ok_or_else(|| anyhow::anyhow!("peer closed during handshake"))??;
    anyhow::ensure!(
        frame.channel == Channel::Control && frame.seq == 0,
        "invalid handshake frame header"
    );
    Ok(frame)
}
fn handshake_material(
    shared: &[u8],
    sid: &str,
    hello: &[u8],
    ack: &[u8],
) -> anyhow::Result<[u8; 128]> {
    let salt = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(sid)?;
    anyhow::ensure!(salt.len() == 32, "invalid session id length");
    let mut context = b"easycontrol-v4-session".to_vec();
    context.extend_from_slice(hello);
    context.extend_from_slice(ack);
    Ok(derive_session_key(shared, &salt, &context))
}
async fn host_spake2_handshake(
    transport: &mut Transport,
    password: Option<&str>,
    session_id: Option<&str>,
) -> anyhow::Result<(String, SessionCipher)> {
    let sid = session_id
        .map(str::to_string)
        .unwrap_or_else(random_session_id);
    let (state, message) = Spake2::<Ed25519Group>::start_b(
        &Password::new(password.unwrap_or("").as_bytes()),
        &Identity::new(CLIENT_IDENTITY),
        &Identity::new(SERVER_IDENTITY),
    );
    let hello = bincode::serialize(&ControlMsg::Hello {
        version: 1,
        protocol_version: PROTOCOL_VERSION,
        session_id: sid.clone(),
        spake2_msg_1: message,
    })?;
    transport
        .send_frame(&Frame {
            channel: Channel::Control,
            seq: 0,
            payload: hello.clone(),
        })
        .await?;
    let ack = read_handshake(transport).await?.payload;
    let peer_message = match bincode::deserialize::<ControlMsg>(&ack)? {
        ControlMsg::HelloAck {
            accepted: true,
            protocol_version: PROTOCOL_VERSION,
            spake2_msg_2,
        } => spake2_msg_2,
        _ => anyhow::bail!("rejected or incompatible HelloAck"),
    };
    let shared = state
        .finish(&peer_message)
        .map_err(|e| anyhow::anyhow!("SPAKE2 host: {}", e))?;
    let material = handshake_material(&shared, &sid, &hello, &ack)?;
    Ok((sid, SessionCipher::for_role(&material, true)))
}
async fn client_spake2_handshake(
    transport: &mut Transport,
    expected_session_id: Option<&str>,
    password: Option<&str>,
) -> anyhow::Result<(String, SessionCipher)> {
    let hello = read_handshake(transport).await?.payload;
    let (sid, peer_message) = match bincode::deserialize::<ControlMsg>(&hello)? {
        ControlMsg::Hello {
            version: 1,
            protocol_version: PROTOCOL_VERSION,
            session_id,
            spake2_msg_1,
        } => (session_id, spake2_msg_1),
        _ => anyhow::bail!("invalid Hello or protocol version mismatch"),
    };
    if let Some(expected) = expected_session_id {
        anyhow::ensure!(expected == sid, "session id mismatch");
    }
    let (state, message) = Spake2::<Ed25519Group>::start_a(
        &Password::new(password.unwrap_or("").as_bytes()),
        &Identity::new(CLIENT_IDENTITY),
        &Identity::new(SERVER_IDENTITY),
    );
    let ack = bincode::serialize(&ControlMsg::HelloAck {
        accepted: true,
        protocol_version: PROTOCOL_VERSION,
        spake2_msg_2: message,
    })?;
    transport
        .send_frame(&Frame {
            channel: Channel::Control,
            seq: 0,
            payload: ack.clone(),
        })
        .await?;
    let shared = state
        .finish(&peer_message)
        .map_err(|e| anyhow::anyhow!("SPAKE2 client: {}", e))?;
    let material = handshake_material(&shared, &sid, &hello, &ack)?;
    Ok((sid, SessionCipher::for_role(&material, false)))
}
async fn confirm_session(
    transport: &mut Transport,
    cipher: &mut SessionCipher,
    is_host: bool,
) -> anyhow::Result<()> {
    let confirm = encrypted_frame(
        cipher,
        1,
        Channel::Control,
        &ControlMsg::Confirm { confirmed: true },
    )?;
    if is_host {
        transport.send_frame(&confirm).await?;
    }
    let frame = transport
        .recv_frame()
        .await?
        .ok_or_else(|| anyhow::anyhow!("peer closed during confirmation"))??;
    anyhow::ensure!(
        frame.channel == Channel::Control,
        "invalid confirmation channel"
    );
    let plain = decrypt_frame(cipher, &frame, &mut 0)?;
    anyhow::ensure!(
        frame.seq == 1
            && matches!(
                bincode::deserialize::<ControlMsg>(&plain)?,
                ControlMsg::Confirm { confirmed: true }
            ),
        "invalid key confirmation"
    );
    if !is_host {
        transport.send_frame(&confirm).await?;
    }
    Ok(())
}

fn cmd_to_frame(
    cipher: &mut SessionCipher,
    seq: u64,
    cmd: &SessionCommand,
) -> anyhow::Result<Frame> {
    match cmd {
        SessionCommand::MouseMove { x, y } => encrypted_frame(
            cipher,
            seq,
            Channel::Input,
            &InputMsg::MouseMove { x: *x, y: *y },
        ),
        SessionCommand::MouseClick { button } => encrypted_frame(
            cipher,
            seq,
            Channel::Input,
            &InputMsg::MouseClick { button: *button },
        ),
        SessionCommand::MousePress { button, x, y } => encrypted_frame(
            cipher,
            seq,
            Channel::Input,
            &InputMsg::MousePress {
                button: *button,
                x: *x,
                y: *y,
            },
        ),
        SessionCommand::MouseRelease { button, x, y } => encrypted_frame(
            cipher,
            seq,
            Channel::Input,
            &InputMsg::MouseRelease {
                button: *button,
                x: *x,
                y: *y,
            },
        ),
        SessionCommand::MouseScroll { dx, dy } => encrypted_frame(
            cipher,
            seq,
            Channel::Input,
            &InputMsg::MouseScroll { dx: *dx, dy: *dy },
        ),
        SessionCommand::KeyPress { code } => encrypted_frame(
            cipher,
            seq,
            Channel::Input,
            &InputMsg::KeyPress { code: *code },
        ),
        SessionCommand::KeyRelease { code } => encrypted_frame(
            cipher,
            seq,
            Channel::Input,
            &InputMsg::KeyRelease { code: *code },
        ),
        SessionCommand::KeyType { text } => encrypted_frame(
            cipher,
            seq,
            Channel::Input,
            &InputMsg::KeyType { text: text.clone() },
        ),
        SessionCommand::ClipboardGet => {
            encrypted_frame(cipher, seq, Channel::Clipboard, &ClipboardMsg::Get)
        }
        SessionCommand::ClipboardSet { text } => encrypted_frame(
            cipher,
            seq,
            Channel::Clipboard,
            &ClipboardMsg::Set {
                data: text.as_bytes().to_vec(),
                mime: "text/plain".into(),
            },
        ),
        SessionCommand::FileList { path } => encrypted_frame(
            cipher,
            seq,
            Channel::File,
            &FileMsg::ListDir { path: path.clone() },
        ),
        SessionCommand::FileRead {
            path,
            offset,
            length,
        } => encrypted_frame(
            cipher,
            seq,
            Channel::File,
            &FileMsg::ReadRequest {
                path: path.clone(),
                offset: *offset,
                length: *length,
            },
        ),
        SessionCommand::FileWrite { path, offset, data } => encrypted_frame(
            cipher,
            seq,
            Channel::File,
            &FileMsg::WriteRequest {
                path: path.clone(),
                offset: *offset,
                data: data.clone(),
            },
        ),
        SessionCommand::ChatMessage { text } => encrypted_frame(
            cipher,
            seq,
            Channel::Chat,
            &ChatMsg::Message {
                from: "peer".into(),
                text: text.clone(),
                ts: 0,
            },
        ),
        SessionCommand::MonitorSelect { index } => encrypted_frame(
            cipher,
            seq,
            Channel::Screen,
            &ScreenMsg::MonitorSelect { index: *index },
        ),
        SessionCommand::Disconnect => encrypted_frame(
            cipher,
            seq,
            Channel::Control,
            &ControlMsg::Bye {
                reason: "local disconnect".into(),
            },
        ),
    }
}

#[cfg(test)]
mod tests;
