use crate::crypto::{derive_session_key, fingerprint, SessionCipher};
#[cfg(feature = "crypto")]
use crate::crypto::{derive_session_key_from_dh, compute_dh_shared_secret, generate_dh_keypair};
use crate::file_transfer::FileSession;
use crate::input::InputSession;
use crate::protocol::{Channel, ChatMsg, ClipboardMsg, ControlMsg, Frame, InputMsg, ScreenMsg};
use crate::relay::{connect_as_client, connect_as_host, RelayServer};
use crate::screen::{default_capture, ScreenSession};
use crate::transport::Transport;
use base64::Engine as _;
use clap::{Parser, Subcommand};
#[cfg(feature = "crypto")]
use getrandom::getrandom;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};
use tokio::task;
use tracing::{error, info, warn};

const SECRET_LEN: usize = 32;

#[derive(Parser)]
#[command(name = "easycontrol")]
#[command(about = "Free, fast remote desktop tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a host that others can connect to
    Host {
        #[arg(long, default_value = "0.0.0.0:5900")]
        listen: SocketAddr,
        #[arg(long)]
        _unattended_password: Option<String>,
        #[arg(long, default_value = "./easycontrol_files")]
        file_root: String,
        #[arg(long)]
        relay: Option<String>,
    },
    /// Connect to a host
    Connect {
        #[arg(long)]
        addr: SocketAddr,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        relay: Option<String>,
    },
    /// Generate a new session identity
    Init,
    /// Run a relay server for NAT traversal
    Relay {
        #[arg(long, default_value = "0.0.0.0:5901")]
        listen: SocketAddr,
    },
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Host { listen, _unattended_password, file_root, relay } => {
            if let Some(relay_addr) = relay {
                host_via_relay(&relay_addr, _unattended_password, file_root).await?;
            } else {
                host(listen, _unattended_password, file_root).await?;
            }
        }
        Commands::Connect { addr, session_id, password, relay } => {
            if let Some(relay_addr) = relay {
                connect_via_relay(&relay_addr, session_id, password).await?;
            } else {
                connect(addr, session_id, password).await?;
            }
        }
        Commands::Init => {
            let mut secret = [0u8; SECRET_LEN];
            #[cfg(feature = "crypto")]
            {
                getrandom(&mut secret).expect("failed to get random bytes");
            }
            #[cfg(not(feature = "crypto"))]
            {
                for (i, b) in secret.iter_mut().enumerate() {
                    *b = i as u8;
                }
            }
            println!("{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&secret));
        }
        Commands::Relay { listen } => {
            let server = RelayServer::new(listen);
            server.run().await?;
        }
    }

    Ok(())
}

async fn host(
    listen: SocketAddr,
    _unattended_password: Option<String>,
    file_root: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(listen).await?;
    info!("easycontrol host listening on {}", listen);
    let file_session = Arc::new(Mutex::new(FileSession::new(&file_root).await?));
    let input_session = Arc::new(Mutex::new(InputSession::default()));

    loop {
        let (stream, peer) = listener.accept().await?;
        info!("connection from {}", peer);

        let file_session = file_session.clone();
        let input_session = input_session.clone();

        let unattended_password = _unattended_password.clone();
        task::spawn(async move {
            if let Err(e) = handle_peer(
                stream,
                peer,
                file_session,
                input_session,
                unattended_password,
            )
            .await
            {
                error!("peer handler error: {}", e);
            }
        });
    }
}

async fn handle_peer(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    _file_session: Arc<Mutex<FileSession>>,
    _input_session: Arc<Mutex<InputSession>>,
    _unattended_password: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut transport = Transport::new(stream);

    // --- Handshake (plaintext, then switch to encrypted after DH) ---
    #[cfg(feature = "crypto")]
    let (host_public, host_private) = generate_dh_keypair();
    #[cfg(not(feature = "crypto"))]
    let host_public: [u8; 32] = [0u8; 32];

    let mut salt = [0u8; 16];
    let mut secret = [0u8; SECRET_LEN];
    #[cfg(feature = "crypto")]
    {
        getrandom(&mut salt).expect("failed to get random bytes");
        getrandom(&mut secret).expect("failed to get random bytes");
    }
    #[cfg(not(feature = "crypto"))]
    {
        for (i, b) in salt.iter_mut().enumerate() {
            *b = i as u8;
        }
        for (i, b) in secret.iter_mut().enumerate() {
            *b = (i + 16) as u8;
        }
    }

    let session_id = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&secret[..8]);

    let cipher: Arc<Mutex<SessionCipher>> = Arc::new(Mutex::new(SessionCipher::new(&[0u8; 128])));

    // Send Hello (plaintext)
    let hello = ControlMsg::Hello { version: 1, session_id: session_id.clone(), public_key: host_public };
    let hello_plain = bincode::serialize(&hello)?;
    let hello_frame = Frame { channel: Channel::Control, seq: 0, payload: hello_plain };
    transport.send_frame(&hello_frame).await?;

    // Receive HelloAck (plaintext)
    let ack_frame = match transport.recv_frame().await? {
        Some(Ok(f)) => f,
        Some(Err(e)) => return Err(e.into()),
        None => return Err("peer closed during handshake".into()),
    };
    let ack_plain = &ack_frame.payload;
    let ack: ControlMsg = bincode::deserialize(ack_plain)?;
    let accepted = match &ack {
        ControlMsg::HelloAck { accepted: true, password, .. } => {
            match &_unattended_password {
                Some(expected_pw) => password.as_deref() == Some(expected_pw.as_str()),
                None => true,
            }
        }
        _ => false,
    };
    if !accepted {
        warn!("connection rejected by peer (bad password or not accepted)");
        return Ok(());
    }

    #[cfg(feature = "crypto")]
    {
        let ControlMsg::HelloAck { public_key: client_public, .. } = &ack else {
            return Err("expected HelloAck with public key".into());
        };
        let shared = compute_dh_shared_secret(&host_private, client_public);
        let key_material = derive_session_key_from_dh(&shared, &salt);
        *cipher.lock().await = SessionCipher::new(&key_material);
    }

    info!("session {} established with {}", session_id, peer);

    // --- Session loop ---
    let (control_tx, mut control_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let next_seq = Arc::new(AtomicU64::new(1));

    let transport = Arc::new(Mutex::new(transport));

    // --- Screen session ---
    let screen_session = Arc::new(Mutex::new(
        ScreenSession::new(default_capture())?,
    ));

    // Send initial monitor list to client
    let monitors = screen_session.lock().await.list_monitors().unwrap_or_default();
    let init = ScreenMsg::MonitorList { monitors };
    let _ = send_channel_msg(
        &transport,
        &cipher,
        &next_seq,
        Channel::Screen,
        &init,
    ).await?;

    // Sender task
    let t_send = transport.clone();
    let next_seq_send = next_seq.clone();
    let send_task = task::spawn(async move {
        while let Some(payload) = control_rx.recv().await {
            let seq = next_seq_send.fetch_add(1, Ordering::Relaxed);
            let frame = Frame { channel: Channel::Control, seq, payload };
            let mut t = t_send.lock().await;
            if let Err(e) = t.send_frame(&frame).await {
                error!("send error: {}", e);
                break;
            }
        }
    });

    // Receiver task
    let t_recv = transport.clone();
    let cipher_recv = cipher.clone();
    let next_seq_recv = next_seq.clone();
    let screen_session_recv = screen_session.clone();
    let (screen_frame_tx, _screen_frame_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let recv_task = task::spawn(async move {
        let mut t = t_recv.lock().await;
        loop {
            match t.recv_frame().await {
                Ok(Some(Ok(frame))) => {
                    let mut c = cipher_recv.lock().await;
                    match c.decrypt(&frame.payload) {
                        Ok(plain) => {
                            match frame.channel {
                                Channel::Control => {
                                    match bincode::deserialize::<ControlMsg>(&plain) {
                                        Ok(ControlMsg::Bye { reason }) => {
                                            info!("peer said bye: {}", reason);
                                            let _ = screen_frame_tx.send(Vec::new());
                                            break;
                                        }
                                        Ok(ControlMsg::Ping) => {
                                            let pong = ControlMsg::Pong;
                                            let pong_plain = bincode::serialize(&pong).unwrap();
                                            let pong_enc = c.encrypt(&pong_plain);
                                            drop(c);
                                            let pong_frame = Frame { channel: Channel::Control, seq: next_seq_recv.fetch_add(1, Ordering::Relaxed), payload: pong_enc };
                                            let _ = t.send_frame(&pong_frame).await;
                                        }
                                        Ok(_) => {}
                                        Err(_) => {}
                                    }
                                }
                                Channel::Clipboard => {
                                    match bincode::deserialize::<ClipboardMsg>(&plain) {
                                        Ok(ClipboardMsg::Get) => {
                                            info!("peer requested clipboard");
                                            drop(c);
                                            let _ = send_clipboard_set(
                                                &t_recv,
                                                &cipher_recv,
                                                &next_seq_recv,
                                                "",
                                            )
                                            .await;
                                        }
                                        Ok(ClipboardMsg::Set { data, .. }) => {
                                            info!("received clipboard data: {} bytes", data.len());
                                        }
                                        Ok(ClipboardMsg::GetAck { .. }) => {
                                            info!("received clipboard get ack");
                                        }
                                        Err(_) => {}
                                    }
                                }
                                Channel::Chat => {
                                    match bincode::deserialize::<ChatMsg>(&plain) {
                                        Ok(ChatMsg::Message { from, text, .. }) => {
                                            info!("chat from {}: {}", from, text);
                                        }
                                        Err(_) => {}
                                    }
                                }
                                Channel::Screen => {
                                    match bincode::deserialize::<ScreenMsg>(&plain) {
                                        Ok(ScreenMsg::MonitorSelect { index }) => {
                                            info!("client requested monitor {}", index);
                                            drop(c);
                                            if let Err(e) = screen_session_recv.lock().await.select_monitor(index) {
                                                warn!("failed to select monitor {}: {}", index, e);
                                            } else {
                                                let monitors = screen_session_recv.lock().await.list_monitors().unwrap_or_default();
                                                let init = ScreenMsg::MonitorList { monitors };
                                                let _ = send_channel_msg(
                                                    &t_recv,
                                                    &cipher_recv,
                                                    &next_seq_recv,
                                                    Channel::Screen,
                                                    &init,
                                                ).await;
                                            }
                                        }
                                        Ok(_) => {}
                                        Err(_) => {}
                                    }
                                }
                                _ => {}
                            }
                        }
                        Err(_) => {}
                    }
                }
                Ok(Some(Err(e))) => {
                    error!("protocol error: {}", e);
                    break;
                }
                Ok(None) => {
                    info!("peer disconnected");
                    let _ = screen_frame_tx.send(Vec::new());
                    break;
                }
                Err(e) => {
                    error!("recv error: {}", e);
                    break;
                }
            }
        }
    });

    // Screen capture loop: periodically capture and send frames to client
    let screen_transport = transport.clone();
    let screen_cipher = cipher.clone();
    let screen_next_seq = next_seq.clone();
    let screen_session_cap = screen_session.clone();
    let screen_task = task::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            interval.tick().await;
            let mut ss = screen_session_cap.lock().await;
            match ss.capture_frame() {
                Ok(data) => {
                    let (w, h) = ss.current_dimensions().unwrap_or((0, 0));
                    let init = ScreenMsg::FrameInit { width: w, height: h, stride: w * 4, format: "RGBA8".into() };
                    let _ = send_channel_msg(
                        &screen_transport,
                        &screen_cipher,
                        &screen_next_seq,
                        Channel::Screen,
                        &init,
                    ).await;
                    let frame_msg = ScreenMsg::FrameData { seq: 0, data, compressed: false };
                    let _ = send_channel_msg(
                        &screen_transport,
                        &screen_cipher,
                        &screen_next_seq,
                        Channel::Screen,
                        &frame_msg,
                    ).await;
                }
                Err(e) => {
                    warn!("screen capture error: {}", e);
                }
            }
        }
    });

    // Drop sender when receiver ends
    let _ = recv_task.await;
    let _ = screen_task.await;
    drop(control_tx);
    let _ = send_task.await;

    Ok(())
}

async fn connect(
    addr: SocketAddr,
    session_id: Option<String>,
    password: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("connecting to {}", addr);
    let stream = tokio::net::TcpStream::connect(addr).await?;
    let mut transport = Transport::new(stream);

    // --- Handshake ---
    #[cfg(feature = "crypto")]
    let (client_public, client_private) = generate_dh_keypair();
    #[cfg(not(feature = "crypto"))]
    let client_public: [u8; 32] = [0u8; 32];

    let hello_frame = match transport.recv_frame().await? {
        Some(Ok(f)) => f,
        Some(Err(e)) => return Err(e.into()),
        None => return Err("host closed during handshake".into()),
    };

    let hello: ControlMsg = bincode::deserialize(&hello_frame.payload)?;
    let (sid, host_public_key) = match hello {
        ControlMsg::Hello { session_id, public_key, .. } => (session_id, public_key),
        _ => return Err("expected Hello from host".into()),
    };

    #[cfg(feature = "crypto")]
    let (shared_secret, key_material) = {
        let shared = compute_dh_shared_secret(&client_private, &host_public_key);
        let km = derive_session_key_from_dh(&shared, &[]);
        (shared, km)
    };
    #[cfg(not(feature = "crypto"))]
    let key_material = derive_session_key(&[], &[]);

    let cipher: Arc<Mutex<SessionCipher>> =
        Arc::new(Mutex::new(SessionCipher::new(&key_material)));

    if let Some(expected) = &session_id {
        if expected != &sid {
            return Err(format!("session id mismatch: expected {}, got {}", expected, sid).into());
        }
    }

    let ack = ControlMsg::HelloAck {
        accepted: true,
        fingerprint: fingerprint(&key_material[..32].try_into().unwrap()),
        public_key: client_public,
        password,
    };
    let ack_plain = bincode::serialize(&ack)?;
    let ack_frame = Frame { channel: Channel::Control, seq: 0, payload: ack_plain };
    transport.send_frame(&ack_frame).await?;

    info!("connected to session {}", sid);

    let transport = Arc::new(Mutex::new(transport));

    // --- Receiver task for incoming frames (screen, clipboard, chat) ---
    let cipher_recv = cipher.clone();
    let t_recv = transport.clone();
    let next_seq_recv = Arc::new(AtomicU64::new(1));
    let (screen_frame_tx, _screen_frame_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let recv_task = task::spawn(async move {
        let mut t = t_recv.lock().await;
        loop {
            match t.recv_frame().await {
                Ok(Some(Ok(frame))) => {
                    let mut c = cipher_recv.lock().await;
                    match c.decrypt(&frame.payload) {
                        Ok(plain) => {
                            match frame.channel {
                                Channel::Control => {
                                    match bincode::deserialize::<ControlMsg>(&plain) {
                                        Ok(ControlMsg::Bye { reason }) => {
                                            info!("peer said bye: {}", reason);
                                            let _ = screen_frame_tx.send(Vec::new());
                                            break;
                                        }
                                        Ok(ControlMsg::Ping) => {
                                            let pong = ControlMsg::Pong;
                                            let pong_plain = bincode::serialize(&pong).unwrap();
                                            let pong_enc = c.encrypt(&pong_plain);
                                            drop(c);
                                            let pong_frame = Frame { channel: Channel::Control, seq: next_seq_recv.fetch_add(1, Ordering::Relaxed), payload: pong_enc };
                                            let _ = t.send_frame(&pong_frame).await;
                                        }
                                        Ok(_) => {}
                                        Err(_) => {}
                                    }
                                }
                                Channel::Clipboard => {
                                    match bincode::deserialize::<ClipboardMsg>(&plain) {
                                        Ok(ClipboardMsg::GetAck { data, mime }) => {
                                            info!("clipboard get ack: {} bytes ({})", data.len(), mime);
                                            if let Ok(text) = String::from_utf8(data.clone()) {
                                                println!("[clipboard] {}", text);
                                            }
                                        }
                                        Ok(_) => {}
                                        Err(_) => {}
                                    }
                                }
                                Channel::Chat => {
                                    match bincode::deserialize::<ChatMsg>(&plain) {
                                        Ok(ChatMsg::Message { from, text, .. }) => {
                                            println!("[chat from {}] {}", from, text);
                                        }
                                        Err(_) => {}
                                    }
                                }
                                _ => {}
                            }
                        }
                        Err(_) => {}
                    }
                }
                Ok(Some(Err(e))) => {
                    error!("protocol error: {}", e);
                    break;
                }
                Ok(None) => {
                    info!("peer disconnected");
                    let _ = screen_frame_tx.send(Vec::new());
                    break;
                }
                Err(e) => {
                    error!("recv error: {}", e);
                    break;
                }
            }
        }
    });

    // --- Interactive loop: read stdin for input commands ---
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();

    println!("easycontrol client. Type commands:");
    println!("  m <x> <y>   - move mouse");
    println!("  c <x> <y>   - click");
    println!("  k <code>    - press key");
    println!("  t <text>    - type text");
    println!("  clipboard get   - get clipboard from host");
    println!("  clipboard set <text>  - set clipboard on host");
    println!("  chat <text>     - send chat message to host");
    println!("  monitor list    - list available monitors on host");
    println!("  monitor select <index>  - select host monitor to view");
    println!("  q           - quit");
    print!("> ");
    io::stdout().flush()?;

    let next_seq = Arc::new(AtomicU64::new(1));

    loop {
        let line = match lines.next_line().await? {
            Some(l) => l,
            None => break,
        };

        let parts: Vec<&str> = line.trim().split_whitespace().collect();
        if parts.is_empty() {
            print!("> ");
            io::stdout().flush()?;
            continue;
        }

        match parts[0] {
            "q" | "quit" => break,
            "m" if parts.len() == 3 => {
                let x: i32 = parts[1].parse().unwrap_or(0);
                let y: i32 = parts[2].parse().unwrap_or(0);
                let msg = InputMsg::MouseMove { x, y };
                send_input(&transport, &cipher, &next_seq, msg).await?;
            }
            "c" if parts.len() == 3 => {
                let x: i32 = parts[1].parse().unwrap_or(0);
                let y: i32 = parts[2].parse().unwrap_or(0);
                send_input(&transport, &cipher, &next_seq, InputMsg::MousePress { button: 1, x, y }).await?;
                send_input(&transport, &cipher, &next_seq, InputMsg::MouseRelease { button: 1, x, y }).await?;
            }
            "k" if parts.len() == 2 => {
                let code: u16 = parts[1].parse().unwrap_or(0);
                send_input(&transport, &cipher, &next_seq, InputMsg::KeyPress { code }).await?;
                send_input(&transport, &cipher, &next_seq, InputMsg::KeyRelease { code }).await?;
            }
            "t" if parts.len() >= 2 => {
                let text = parts[1..].join(" ");
                send_input(&transport, &cipher, &next_seq, InputMsg::KeyType { text }).await?;
            }
            "clipboard" => {
                if parts.len() >= 2 && parts[1] == "get" {
                    send_clipboard_get(&transport, &cipher, &next_seq).await?;
                } else if parts.len() >= 3 && parts[1] == "set" {
                    let text = parts[2..].join(" ");
                    send_clipboard_set(&transport, &cipher, &next_seq, &text).await?;
                } else {
                    println!("usage: clipboard get | clipboard set <text>");
                }
            }
            "chat" if parts.len() >= 2 => {
                let text = parts[1..].join(" ");
                send_chat_message(&transport, &cipher, &next_seq, "client", &text).await?;
            }
            "monitor" => {
                if parts.len() >= 2 && parts[1] == "list" {
                    send_channel_msg(
                        &transport,
                        &cipher,
                        &next_seq,
                        Channel::Screen,
                        &ScreenMsg::MonitorSelect { index: 0 },
                    )
                    .await?;
                } else if parts.len() >= 3 && parts[1] == "select" {
                    let index: usize = parts[2].parse().unwrap_or(0);
                    send_channel_msg(
                        &transport,
                        &cipher,
                        &next_seq,
                        Channel::Screen,
                        &ScreenMsg::MonitorSelect { index },
                    )
                    .await?;
                } else {
                    println!("usage: monitor list | monitor select <index>");
                }
            }
            _ => {
                println!("unknown command");
            }
        }
        print!("> ");
        io::stdout().flush()?;
    }

    // Send Bye
    let bye = ControlMsg::Bye { reason: "client disconnect".into() };
    let bye_plain = bincode::serialize(&bye)?;
    let bye_enc = {
        let mut c = cipher.lock().await;
        c.encrypt(&bye_plain)
    };
    let bye_frame = Frame { channel: Channel::Control, seq: next_seq.fetch_add(1, Ordering::Relaxed), payload: bye_enc };
    let mut t = transport.lock().await;
    let _ = t.send_frame(&bye_frame).await;

    // Wait for receiver task to finish
    let _ = recv_task.await;

    Ok(())
}

async fn host_via_relay(
    relay_addr: &str,
    _unattended_password: Option<String>,
    file_root: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut secret = [0u8; SECRET_LEN];
    #[cfg(feature = "crypto")]
    {
        getrandom(&mut secret).expect("failed to get random bytes");
    }
    #[cfg(not(feature = "crypto"))]
    {
        for (i, b) in secret.iter_mut().enumerate() {
            *b = i as u8;
        }
    }
    let session_id = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&secret[..8]);
    info!("easycontrol host via relay, session id: {}", session_id);

    let stream = connect_as_host(relay_addr, &session_id).await?;
    info!("registered with relay at {}", relay_addr);

    let file_session = Arc::new(Mutex::new(FileSession::new(&file_root).await?));
    let input_session = Arc::new(Mutex::new(InputSession::default()));

    handle_peer(stream, "relay".parse().unwrap(), file_session, input_session, _unattended_password).await?;

    Ok(())
}

async fn connect_via_relay(
    relay_addr: &str,
    session_id: Option<String>,
    password: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let target_session = session_id.clone().ok_or("session id required for relay mode")?;
    info!("connecting to relay {} for session {}", relay_addr, target_session);

    let stream = connect_as_client(relay_addr, &target_session).await?;
    info!("connected to relay, waiting for host...");

    let mut transport = Transport::new(stream);

    // --- Handshake (plaintext, then switch to encrypted after DH) ---
    #[cfg(feature = "crypto")]
    let (client_public, client_private) = generate_dh_keypair();
    #[cfg(not(feature = "crypto"))]
    let client_public: [u8; 32] = [0u8; 32];

    let hello_frame = match transport.recv_frame().await? {
        Some(Ok(f)) => f,
        Some(Err(e)) => return Err(e.into()),
        None => return Err("host closed during handshake".into()),
    };

    let hello: ControlMsg = bincode::deserialize(&hello_frame.payload)?;
    let (sid, host_public_key) = match hello {
        ControlMsg::Hello { session_id, public_key, .. } => (session_id, public_key),
        _ => return Err("expected Hello from host".into()),
    };

    #[cfg(feature = "crypto")]
    let (shared_secret, key_material) = {
        let shared = compute_dh_shared_secret(&client_private, &host_public_key);
        let km = derive_session_key_from_dh(&shared, &[]);
        (shared, km)
    };
    #[cfg(not(feature = "crypto"))]
    let key_material = derive_session_key(&[], &[]);

    let cipher: Arc<Mutex<SessionCipher>> =
        Arc::new(Mutex::new(SessionCipher::new(&key_material)));

    if let Some(expected) = &session_id {
        if expected != &sid {
            return Err(format!("session id mismatch: expected {}, got {}", expected, sid).into());
        }
    }

    let ack = ControlMsg::HelloAck {
        accepted: true,
        fingerprint: fingerprint(&key_material[..32].try_into().unwrap()),
        public_key: client_public,
        password,
    };
    let ack_plain = bincode::serialize(&ack)?;
    let ack_frame = Frame { channel: Channel::Control, seq: 0, payload: ack_plain };
    transport.send_frame(&ack_frame).await?;

    info!("connected to session {}", sid);

    // --- Interactive loop ---
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();

    println!("easycontrol client. Type commands:");
    println!("  m <x> <y>   - move mouse");
    println!("  c <x> <y>   - click");
    println!("  k <code>    - press key");
    println!("  t <text>    - type text");
    println!("  q           - quit");
    print!("> ");
    io::stdout().flush()?;

    let next_seq = Arc::new(AtomicU64::new(1));
    let transport = Arc::new(Mutex::new(transport));

    loop {
        let line = match lines.next_line().await? {
            Some(l) => l,
            None => break,
        };

        let parts: Vec<&str> = line.trim().split_whitespace().collect();
        if parts.is_empty() {
            print!("> ");
            io::stdout().flush()?;
            continue;
        }

        match parts[0] {
            "q" | "quit" => break,
            "m" if parts.len() == 3 => {
                let x: i32 = parts[1].parse().unwrap_or(0);
                let y: i32 = parts[2].parse().unwrap_or(0);
                let msg = InputMsg::MouseMove { x, y };
                send_input(&transport, &cipher, &next_seq, msg).await?;
            }
            "c" if parts.len() == 3 => {
                let x: i32 = parts[1].parse().unwrap_or(0);
                let y: i32 = parts[2].parse().unwrap_or(0);
                send_input(&transport, &cipher, &next_seq, InputMsg::MousePress { button: 1, x, y }).await?;
                send_input(&transport, &cipher, &next_seq, InputMsg::MouseRelease { button: 1, x, y }).await?;
            }
            "k" if parts.len() == 2 => {
                let code: u16 = parts[1].parse().unwrap_or(0);
                send_input(&transport, &cipher, &next_seq, InputMsg::KeyPress { code }).await?;
                send_input(&transport, &cipher, &next_seq, InputMsg::KeyRelease { code }).await?;
            }
            "t" if parts.len() >= 2 => {
                let text = parts[1..].join(" ");
                send_input(&transport, &cipher, &next_seq, InputMsg::KeyType { text }).await?;
            }
            _ => {
                println!("unknown command");
            }
        }
        print!("> ");
        io::stdout().flush()?;
    }

    // Send Bye
    let bye = ControlMsg::Bye { reason: "client disconnect".into() };
    let bye_plain = bincode::serialize(&bye)?;
    let bye_enc = {
        let mut c = cipher.lock().await;
        c.encrypt(&bye_plain)
    };
    let bye_frame = Frame { channel: Channel::Control, seq: next_seq.fetch_add(1, Ordering::Relaxed), payload: bye_enc };
    let mut t = transport.lock().await;
    let _ = t.send_frame(&bye_frame).await;

    Ok(())
}

async fn send_input(
    transport: &Arc<Mutex<Transport>>,
    cipher: &Arc<Mutex<SessionCipher>>,
    next_seq: &Arc<AtomicU64>,
    msg: InputMsg,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut c = cipher.lock().await;
    let plain = bincode::serialize(&msg)?;
    let enc = c.encrypt(&plain);
    let seq = next_seq.fetch_add(1, Ordering::Relaxed);
    let frame = Frame { channel: Channel::Input, seq, payload: enc };
    let mut t = transport.lock().await;
    t.send_frame(&frame).await?;
    Ok(())
}

async fn send_channel_msg<T: serde::Serialize + Clone>(
    transport: &Arc<Mutex<Transport>>,
    cipher: &Arc<Mutex<SessionCipher>>,
    next_seq: &Arc<AtomicU64>,
    channel: Channel,
    msg: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut c = cipher.lock().await;
    let plain = bincode::serialize(msg)?;
    let enc = c.encrypt(&plain);
    let seq = next_seq.fetch_add(1, Ordering::Relaxed);
    let frame = Frame { channel, seq, payload: enc };
    let mut t = transport.lock().await;
    t.send_frame(&frame).await?;
    Ok(())
}

async fn send_clipboard_get(
    transport: &Arc<Mutex<Transport>>,
    cipher: &Arc<Mutex<SessionCipher>>,
    next_seq: &Arc<AtomicU64>,
) -> Result<(), Box<dyn std::error::Error>> {
    send_channel_msg(transport, cipher, next_seq, Channel::Clipboard, &ClipboardMsg::Get).await
}

async fn send_clipboard_set(
    transport: &Arc<Mutex<Transport>>,
    cipher: &Arc<Mutex<SessionCipher>>,
    next_seq: &Arc<AtomicU64>,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    send_channel_msg(
        transport,
        cipher,
        next_seq,
        Channel::Clipboard,
        &ClipboardMsg::Set { data: text.as_bytes().to_vec(), mime: "text/plain".into() },
    )
    .await
}

async fn send_chat_message(
    transport: &Arc<Mutex<Transport>>,
    cipher: &Arc<Mutex<SessionCipher>>,
    next_seq: &Arc<AtomicU64>,
    from: &str,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    send_channel_msg(
        transport,
        cipher,
        next_seq,
        Channel::Chat,
        &ChatMsg::Message { from: from.into(), text: text.into(), ts: 0 },
    )
    .await
}

