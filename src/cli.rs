use crate::crypto::{derive_session_key, SessionCipher};
use crate::file_transfer::FileSession;
use crate::input::InputSession;
use crate::protocol::{
    Channel, ChatMsg, ClipboardMsg, ControlMsg, Frame, InputMsg, ScreenMsg, PROTOCOL_VERSION,
};
use crate::relay::{connect_as_client, connect_as_host, RelayServer};
use crate::screen::{default_capture, ScreenSession};
use crate::transport::Transport;
use base64::Engine as _;
use clap::{Parser, Subcommand};
use getrandom::getrandom;
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::io::{self, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};
use tokio::task;
use tracing::{error, info, warn};

const SECRET_LEN: usize = 32;
const SEQ_PREFIX_LEN: usize = 8; // First 8 bytes of plaintext are LE u64 sequence number for replay protection

/// SPAKE2 identity strings for role separation.
const CLIENT_IDENTITY: &[u8] = b"easycontrol-v3-client";
const SERVER_IDENTITY: &[u8] = b"easycontrol-v3-host";
const SPAKE2_SUITE: Ed25519Group = Ed25519Group;

/// Encrypt a message with an embedded sequence number for replay protection.
/// The first 8 bytes of plaintext are the LE u64 sequence number.
fn encrypt_with_seq(cipher: &mut SessionCipher, seq: u64, plaintext: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(SEQ_PREFIX_LEN + plaintext.len());
    buf.extend_from_slice(&seq.to_le_bytes());
    buf.extend_from_slice(plaintext);
    cipher.encrypt(&buf)
}

/// Decrypt a message and verify sequence number for replay protection.
/// cipher.decrypt() returns plaintext with the 8-byte sequence prefix stripped.
fn decrypt_with_seq(
    cipher: &mut SessionCipher,
    ciphertext: &[u8],
) -> Result<Vec<u8>, crate::crypto::CryptoError> {
    cipher.decrypt(ciphertext)
}

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
        unattended_password: Option<String>,
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
        Commands::Host {
            listen,
            unattended_password,
            file_root,
            relay,
        } => {
            if let Some(relay_addr) = relay {
                host_via_relay(&relay_addr, unattended_password, file_root).await?;
            } else {
                host(listen, unattended_password, file_root).await?;
            }
        }
        Commands::Connect {
            addr,
            session_id,
            password,
            relay,
        } => {
            if let Some(relay_addr) = relay {
                connect_via_relay(&relay_addr, session_id, password).await?;
            } else {
                connect(addr, session_id, password).await?;
            }
        }
        Commands::Init => {
            let mut secret = [0u8; SECRET_LEN];
            getrandom(&mut secret).expect("failed to get random bytes");
            println!(
                "{}",
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&secret)
            );
        }
        Commands::Relay { listen } => {
            let server = RelayServer::new(listen);
            server.run().await?;
        }
    }

    Ok(())
}

/// Perform a SPAKE2 PAKE handshake on the host (server) side.
/// Returns (session_id, session_cipher) on success.
async fn host_spake2_handshake(
    transport: &mut Transport,
    password: Option<&str>,
) -> Result<(String, SessionCipher), Box<dyn std::error::Error>> {
    // Generate session ID
    let mut session_id_bytes = [0u8; 32];
    getrandom(&mut session_id_bytes).expect("failed to get random bytes");
    let session_id = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&session_id_bytes);

    // Create SPAKE2 server state (role B) and generate message 1
    let password_bytes = password.unwrap_or("").as_bytes();
    let password = Password::new(password_bytes);
    let client_id = Identity::new(CLIENT_IDENTITY);
    let server_id = Identity::new(SERVER_IDENTITY);
    let (mut server_state, server_msg1) =
        Spake2::<Ed25519Group>::start_b(&password, &client_id, &server_id);

    // Send Hello (plaintext header - no secrets)
    let hello = ControlMsg::Hello {
        version: 1,
        protocol_version: PROTOCOL_VERSION,
        session_id: session_id.clone(),
        spake2_msg_1: server_msg1,
    };
    let hello_plain = bincode::serialize(&hello)?;
    let hello_frame = Frame {
        channel: Channel::Control,
        seq: 0,
        payload: hello_plain,
    };
    transport.send_frame(&hello_frame).await?;

    // Receive HelloAck (plaintext header)
    let ack_frame = match transport.recv_frame().await? {
        Some(Ok(f)) => f,
        Some(Err(e)) => return Err(e.into()),
        None => return Err("peer closed during handshake".into()),
    };
    let ack: ControlMsg = bincode::deserialize(&ack_frame.payload)?;

    // Verify protocol version
    match &ack {
        ControlMsg::HelloAck {
            protocol_version: pv,
            ..
        } if *pv != PROTOCOL_VERSION => {
            warn!(
                "protocol version mismatch: expected {}, got {}",
                PROTOCOL_VERSION, pv
            );
            return Err("protocol version mismatch".into());
        }
        _ => {}
    }

    let accepted = matches!(&ack, ControlMsg::HelloAck { accepted: true, .. });
    if !accepted {
        warn!("connection rejected by peer");
        return Err("connection rejected by peer".into());
    }

    // Finish SPAKE2: derive shared key from client's message 2
    let client_msg2 = match &ack {
        ControlMsg::HelloAck { spake2_msg_2, .. } => spake2_msg_2.clone(),
        _ => return Err("expected HelloAck with SPAKE2 message 2".into()),
    };

    let shared_key = server_state.finish(&client_msg2).map_err(|e| {
        error!("SPAKE2 server finish failed: {}", e);
        e.to_string()
    })?;

    // Derive session key material from SPAKE2 shared key + session ID
    let key_material =
        derive_session_key(&shared_key, &session_id_bytes, b"easycontrol-v3-session");

    let cipher = SessionCipher::new(&key_material);

    Ok((session_id, cipher))
}

/// Perform a SPAKE2 PAKE handshake on the client (client) side.
/// Returns (session_id, session_cipher) on success.
async fn client_spake2_handshake(
    transport: &mut Transport,
    expected_session_id: Option<&str>,
    password: Option<&str>,
) -> Result<(String, SessionCipher), Box<dyn std::error::Error>> {
    // Receive Hello (plaintext header)
    let hello_frame = match transport.recv_frame().await? {
        Some(Ok(f)) => f,
        Some(Err(e)) => return Err(e.into()),
        None => return Err("host closed during handshake".into()),
    };

    let hello: ControlMsg = bincode::deserialize(&hello_frame.payload)?;
    let (sid, host_spake2_msg1) = match hello {
        ControlMsg::Hello {
            session_id,
            spake2_msg_1,
            protocol_version,
            ..
        } => {
            if protocol_version != PROTOCOL_VERSION {
                return Err(format!(
                    "protocol version mismatch: expected {}, got {}",
                    PROTOCOL_VERSION, protocol_version
                )
                .into());
            }
            (session_id, spake2_msg_1)
        }
        _ => return Err("expected Hello from host".into()),
    };

    if let Some(expected) = expected_session_id {
        if expected != &sid {
            return Err(format!("session id mismatch: expected {}, got {}", expected, sid).into());
        }
    }

    // Create SPAKE2 client state and generate message 1
    let password_bytes = password.unwrap_or("").as_bytes();
    let password = Password::new(password_bytes);
    let client_id = Identity::new(CLIENT_IDENTITY);
    let server_id = Identity::new(SERVER_IDENTITY);
    let (client_state, client_msg1) =
        Spake2::<Ed25519Group>::start_a(&password, &client_id, &server_id);

    // Send HelloAck (plaintext header) with SPAKE2 message 1
    let ack = ControlMsg::HelloAck {
        accepted: true,
        spake2_msg_2: client_msg1,
        protocol_version: PROTOCOL_VERSION,
    };
    let ack_plain = bincode::serialize(&ack)?;
    let ack_frame = Frame {
        channel: Channel::Control,
        seq: 0,
        payload: ack_plain,
    };
    transport.send_frame(&ack_frame).await?;

    // Finish SPAKE2: derive shared key from host's message 2
    let shared_key = client_state.finish(&host_spake2_msg1).map_err(|e| {
        error!("SPAKE2 client finish failed: {}", e);
        e.to_string()
    })?;

    // Derive session key material from SPAKE2 shared key + session ID
    let key_material = derive_session_key(&shared_key, sid.as_bytes(), b"easycontrol-v3-session");

    let cipher = SessionCipher::new(&key_material);

    Ok((sid, cipher))
}

/// Exchange encrypted confirmations to prove both sides derived the same key.
/// Returns Ok(()) if both sides confirmed, Err if confirmation failed.
async fn confirm_session(
    transport: &Arc<Mutex<Transport>>,
    cipher: &Arc<Mutex<SessionCipher>>,
    next_seq: &Arc<AtomicU64>,
    am_host: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if am_host {
        // Host sends confirmation first
        let confirm = ControlMsg::Confirm { confirmed: true };
        let seq = next_seq.fetch_add(1, Ordering::Relaxed);
        let mut c = cipher.lock().await;
        let enc = encrypt_with_seq(&mut c, seq, &bincode::serialize(&confirm)?);
        let frame = Frame {
            channel: Channel::Control,
            seq,
            payload: enc,
        };
        transport.lock().await.send_frame(&frame).await?;

        // Wait for client's confirmation
        let mut t = transport.lock().await;
        #[allow(clippy::never_loop)]
        loop {
            match t.recv_frame().await {
                Ok(Some(Ok(frame))) => {
                    let mut c = cipher.lock().await;
                    match decrypt_with_seq(&mut c, &frame.payload) {
                        Ok(plain) => match bincode::deserialize::<ControlMsg>(&plain) {
                            Ok(ControlMsg::Confirm { confirmed }) => {
                                if confirmed {
                                    return Ok(());
                                }
                                return Err("peer confirmation rejected".into());
                            }
                            _ => return Err("unexpected message during confirmation".into()),
                        },
                        Err(_) => return Err("failed to decrypt confirmation".into()),
                    }
                }
                Ok(Some(Err(e))) => return Err(e.into()),
                Ok(None) => return Err("peer closed during confirmation".into()),
                Err(e) => return Err(e.into()),
            }
        }
    } else {
        // Client waits for host's confirmation first
        let mut t = transport.lock().await;
        #[allow(clippy::never_loop)]
        loop {
            match t.recv_frame().await {
                Ok(Some(Ok(frame))) => {
                    let mut c = cipher.lock().await;
                    match decrypt_with_seq(&mut c, &frame.payload) {
                        Ok(plain) => {
                            match bincode::deserialize::<ControlMsg>(&plain) {
                                Ok(ControlMsg::Confirm { confirmed }) => {
                                    if confirmed {
                                        // Send our confirmation back
                                        drop(c);
                                        let confirm = ControlMsg::Confirm { confirmed: true };
                                        let seq = next_seq.fetch_add(1, Ordering::Relaxed);
                                        let mut c = cipher.lock().await;
                                        let enc = encrypt_with_seq(
                                            &mut c,
                                            seq,
                                            &bincode::serialize(&confirm)?,
                                        );
                                        let frame = Frame {
                                            channel: Channel::Control,
                                            seq,
                                            payload: enc,
                                        };
                                        transport.lock().await.send_frame(&frame).await?;
                                        return Ok(());
                                    }
                                    return Err("peer confirmation rejected".into());
                                }
                                _ => return Err("unexpected message during confirmation".into()),
                            }
                        }
                        Err(_) => return Err("failed to decrypt confirmation".into()),
                    }
                }
                Ok(Some(Err(e))) => return Err(e.into()),
                Ok(None) => return Err("peer closed during confirmation".into()),
                Err(e) => return Err(e.into()),
            }
        }
    }
}

async fn host(
    listen: SocketAddr,
    unattended_password: Option<String>,
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
        let unattended_password = unattended_password.clone();

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

    // --- SPAKE2 Handshake ---
    let (session_id, cipher) =
        host_spake2_handshake(&mut transport, _unattended_password.as_deref()).await?;

    let cipher: Arc<Mutex<SessionCipher>> = Arc::new(Mutex::new(cipher));
    let transport = Arc::new(Mutex::new(transport));

    // --- Screen session ---
    let screen_session = Arc::new(Mutex::new(ScreenSession::new(default_capture())?));

    // Exchange encrypted confirmations to prove both sides derived the same key
    confirm_session(&transport, &cipher, &Arc::new(AtomicU64::new(1)), true).await?;

    info!("session {} authenticated with {}", session_id, peer);

    // --- Session loop ---
    let (control_tx, mut control_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let next_seq = Arc::new(AtomicU64::new(1));

    // Sender task
    let t_send = transport.clone();
    let next_seq_send = next_seq.clone();
    let send_task = task::spawn(async move {
        while let Some(payload) = control_rx.recv().await {
            let seq = next_seq_send.fetch_add(1, Ordering::Relaxed);
            let frame = Frame {
                channel: Channel::Control,
                seq,
                payload,
            };
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
                    match decrypt_with_seq(&mut c, &frame.payload) {
                        Ok(plain) => match frame.channel {
                            Channel::Control => match bincode::deserialize::<ControlMsg>(&plain) {
                                Ok(ControlMsg::Bye { reason }) => {
                                    info!("peer said bye: {}", reason);
                                    let _ = screen_frame_tx.send(Vec::new());
                                    break;
                                }
                                Ok(ControlMsg::Ping) => {
                                    let pong = ControlMsg::Pong;
                                    let seq = next_seq_recv.fetch_add(1, Ordering::Relaxed);
                                    let pong_plain = bincode::serialize(&pong).unwrap();
                                    let pong_enc = encrypt_with_seq(&mut c, seq, &pong_plain);
                                    drop(c);
                                    let pong_frame = Frame {
                                        channel: Channel::Control,
                                        seq,
                                        payload: pong_enc,
                                    };
                                    let _ = t.send_frame(&pong_frame).await;
                                }
                                Ok(_) => {}
                                Err(_) => {}
                            },
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
                            Channel::Chat => match bincode::deserialize::<ChatMsg>(&plain) {
                                Ok(ChatMsg::Message { from, text, .. }) => {
                                    info!("chat from {}: {}", from, text);
                                }
                                Err(_) => {}
                            },
                            Channel::Screen => match bincode::deserialize::<ScreenMsg>(&plain) {
                                Ok(ScreenMsg::MonitorSelect { index }) => {
                                    info!("client requested monitor {}", index);
                                    drop(c);
                                    if let Err(e) =
                                        screen_session_recv.lock().await.select_monitor(index)
                                    {
                                        warn!("failed to select monitor {}: {}", index, e);
                                    } else {
                                        let monitors = screen_session_recv
                                            .lock()
                                            .await
                                            .list_monitors()
                                            .unwrap_or_default();
                                        let init = ScreenMsg::MonitorList { monitors };
                                        let _ = send_channel_msg(
                                            &t_recv,
                                            &cipher_recv,
                                            &next_seq_recv,
                                            Channel::Screen,
                                            &init,
                                        )
                                        .await;
                                    }
                                }
                                Ok(_) => {}
                                Err(_) => {}
                            },
                            _ => {}
                        },
                        Err(crypto_err) => {
                            warn!("crypto error on frame from {}: {}", peer, crypto_err);
                            break;
                        }
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
                    let init = ScreenMsg::FrameInit {
                        width: w,
                        height: h,
                        stride: w * 4,
                        format: "RGBA8".into(),
                    };
                    let seq = screen_next_seq.fetch_add(1, Ordering::Relaxed);
                    let _ = send_channel_msg(
                        &screen_transport,
                        &screen_cipher,
                        &screen_next_seq,
                        Channel::Screen,
                        &init,
                    )
                    .await;
                    let seq2 = screen_next_seq.fetch_add(1, Ordering::Relaxed);
                    let frame_msg = ScreenMsg::FrameData {
                        seq: seq2,
                        data,
                        compressed: false,
                    };
                    let _ = send_channel_msg(
                        &screen_transport,
                        &screen_cipher,
                        &screen_next_seq,
                        Channel::Screen,
                        &frame_msg,
                    )
                    .await;
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

    // --- SPAKE2 Handshake ---
    let (session_id, cipher) =
        client_spake2_handshake(&mut transport, session_id.as_deref(), password.as_deref()).await?;

    let cipher: Arc<Mutex<SessionCipher>> = Arc::new(Mutex::new(cipher));
    let transport = Arc::new(Mutex::new(transport));

    // Exchange encrypted confirmations
    confirm_session(&transport, &cipher, &Arc::new(AtomicU64::new(1)), false).await?;

    info!("connected to session {}", session_id);

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
                    match decrypt_with_seq(&mut c, &frame.payload) {
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
                                            let seq = next_seq_recv.fetch_add(1, Ordering::Relaxed);
                                            let pong_plain = bincode::serialize(&pong).unwrap();
                                            let pong_enc =
                                                encrypt_with_seq(&mut c, seq, &pong_plain);
                                            drop(c);
                                            let pong_frame = Frame {
                                                channel: Channel::Control,
                                                seq,
                                                payload: pong_enc,
                                            };
                                            let _ = t.send_frame(&pong_frame).await;
                                        }
                                        Ok(_) => {}
                                        Err(_) => {}
                                    }
                                }
                                Channel::Clipboard => {
                                    match bincode::deserialize::<ClipboardMsg>(&plain) {
                                        Ok(ClipboardMsg::GetAck { data, mime }) => {
                                            info!(
                                                "clipboard get ack: {} bytes ({})",
                                                data.len(),
                                                mime
                                            );
                                            // Never log clipboard contents
                                        }
                                        Ok(_) => {}
                                        Err(_) => {}
                                    }
                                }
                                Channel::Chat => match bincode::deserialize::<ChatMsg>(&plain) {
                                    Ok(ChatMsg::Message { from, text, .. }) => {
                                        println!("[chat from {}] {}", from, text);
                                    }
                                    Err(_) => {}
                                },
                                _ => {}
                            }
                        }
                        Err(crypto_err) => {
                            warn!("crypto error on frame: {}", crypto_err);
                            break;
                        }
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
                send_input(
                    &transport,
                    &cipher,
                    &next_seq,
                    InputMsg::MousePress { button: 1, x, y },
                )
                .await?;
                send_input(
                    &transport,
                    &cipher,
                    &next_seq,
                    InputMsg::MouseRelease { button: 1, x, y },
                )
                .await?;
            }
            "k" if parts.len() == 2 => {
                let code: u16 = parts[1].parse().unwrap_or(0);
                send_input(&transport, &cipher, &next_seq, InputMsg::KeyPress { code }).await?;
                send_input(
                    &transport,
                    &cipher,
                    &next_seq,
                    InputMsg::KeyRelease { code },
                )
                .await?;
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
    let bye = ControlMsg::Bye {
        reason: "client disconnect".into(),
    };
    let bye_plain = bincode::serialize(&bye)?;
    let bye_enc = {
        let mut c = cipher.lock().await;
        encrypt_with_seq(&mut c, next_seq.fetch_add(1, Ordering::Relaxed), &bye_plain)
    };
    let bye_frame = Frame {
        channel: Channel::Control,
        seq: next_seq.fetch_add(1, Ordering::Relaxed),
        payload: bye_enc,
    };
    let mut t = transport.lock().await;
    let _ = t.send_frame(&bye_frame).await;

    // Wait for receiver task to finish
    let _ = recv_task.await;

    Ok(())
}

async fn host_via_relay(
    relay_addr: &str,
    unattended_password: Option<String>,
    file_root: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut secret = [0u8; SECRET_LEN];
    getrandom(&mut secret).expect("failed to get random bytes");
    let session_id = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&secret[..8]);
    info!("easycontrol host via relay, session id: {}", session_id);

    let stream = connect_as_host(relay_addr, &session_id).await?;
    info!("registered with relay at {}", relay_addr);

    let file_session = Arc::new(Mutex::new(FileSession::new(&file_root).await?));
    let input_session = Arc::new(Mutex::new(InputSession::default()));

    handle_peer(
        stream,
        "relay".parse().unwrap(),
        file_session,
        input_session,
        unattended_password,
    )
    .await?;

    Ok(())
}

async fn connect_via_relay(
    relay_addr: &str,
    session_id: Option<String>,
    password: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let target_session = session_id
        .clone()
        .ok_or("session id required for relay mode")?;
    info!(
        "connecting to relay {} for session {}",
        relay_addr, target_session
    );

    let stream = connect_as_client(relay_addr, &target_session).await?;
    info!("connected to relay, waiting for host...");

    let mut transport = Transport::new(stream);

    // --- SPAKE2 Handshake ---
    let (session_id, cipher) =
        client_spake2_handshake(&mut transport, session_id.as_deref(), password.as_deref()).await?;

    let cipher: Arc<Mutex<SessionCipher>> = Arc::new(Mutex::new(cipher));
    let transport = Arc::new(Mutex::new(transport));

    // Exchange encrypted confirmations
    confirm_session(&transport, &cipher, &Arc::new(AtomicU64::new(1)), false).await?;

    info!("connected to session {}", session_id);

    // --- Receiver task ---
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
                    match decrypt_with_seq(&mut c, &frame.payload) {
                        Ok(plain) => match frame.channel {
                            Channel::Control => match bincode::deserialize::<ControlMsg>(&plain) {
                                Ok(ControlMsg::Bye { reason }) => {
                                    info!("peer said bye: {}", reason);
                                    let _ = screen_frame_tx.send(Vec::new());
                                    break;
                                }
                                Ok(ControlMsg::Ping) => {
                                    let pong = ControlMsg::Pong;
                                    let seq = next_seq_recv.fetch_add(1, Ordering::Relaxed);
                                    let pong_plain = bincode::serialize(&pong).unwrap();
                                    let pong_enc = encrypt_with_seq(&mut c, seq, &pong_plain);
                                    drop(c);
                                    let pong_frame = Frame {
                                        channel: Channel::Control,
                                        seq,
                                        payload: pong_enc,
                                    };
                                    let _ = t.send_frame(&pong_frame).await;
                                }
                                Ok(_) => {}
                                Err(_) => {}
                            },
                            Channel::Clipboard => {
                                match bincode::deserialize::<ClipboardMsg>(&plain) {
                                    Ok(ClipboardMsg::GetAck { data, .. }) => {
                                        info!("clipboard get ack: {} bytes", data.len());
                                    }
                                    Ok(_) => {}
                                    Err(_) => {}
                                }
                            }
                            Channel::Chat => match bincode::deserialize::<ChatMsg>(&plain) {
                                Ok(ChatMsg::Message { from, text, .. }) => {
                                    println!("[chat from {}] {}", from, text);
                                }
                                Err(_) => {}
                            },
                            _ => {}
                        },
                        Err(crypto_err) => {
                            warn!("crypto error on frame: {}", crypto_err);
                            break;
                        }
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
                send_input(
                    &transport,
                    &cipher,
                    &next_seq,
                    InputMsg::MousePress { button: 1, x, y },
                )
                .await?;
                send_input(
                    &transport,
                    &cipher,
                    &next_seq,
                    InputMsg::MouseRelease { button: 1, x, y },
                )
                .await?;
            }
            "k" if parts.len() == 2 => {
                let code: u16 = parts[1].parse().unwrap_or(0);
                send_input(&transport, &cipher, &next_seq, InputMsg::KeyPress { code }).await?;
                send_input(
                    &transport,
                    &cipher,
                    &next_seq,
                    InputMsg::KeyRelease { code },
                )
                .await?;
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
    let bye = ControlMsg::Bye {
        reason: "client disconnect".into(),
    };
    let bye_plain = bincode::serialize(&bye)?;
    let bye_enc = {
        let mut c = cipher.lock().await;
        encrypt_with_seq(&mut c, next_seq.fetch_add(1, Ordering::Relaxed), &bye_plain)
    };
    let bye_frame = Frame {
        channel: Channel::Control,
        seq: next_seq.fetch_add(1, Ordering::Relaxed),
        payload: bye_enc,
    };
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
    let seq = next_seq.fetch_add(1, Ordering::Relaxed);
    let enc = encrypt_with_seq(&mut c, seq, &plain);
    let frame = Frame {
        channel: Channel::Input,
        seq,
        payload: enc,
    };
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
    let seq = next_seq.fetch_add(1, Ordering::Relaxed);
    let enc = encrypt_with_seq(&mut c, seq, &plain);
    let frame = Frame {
        channel,
        seq,
        payload: enc,
    };
    let mut t = transport.lock().await;
    t.send_frame(&frame).await?;
    Ok(())
}

async fn send_clipboard_get(
    transport: &Arc<Mutex<Transport>>,
    cipher: &Arc<Mutex<SessionCipher>>,
    next_seq: &Arc<AtomicU64>,
) -> Result<(), Box<dyn std::error::Error>> {
    send_channel_msg(
        transport,
        cipher,
        next_seq,
        Channel::Clipboard,
        &ClipboardMsg::Get,
    )
    .await
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
        &ClipboardMsg::Set {
            data: text.as_bytes().to_vec(),
            mime: "text/plain".into(),
        },
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
        &ChatMsg::Message {
            from: from.into(),
            text: text.into(),
            ts: 0,
        },
    )
    .await
}
