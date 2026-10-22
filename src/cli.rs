use crate::crypto::{derive_session_key, fingerprint, SessionCipher};
#[cfg(feature = "crypto")]
use crate::crypto::{derive_session_key_from_dh, compute_dh_shared_secret, generate_dh_keypair};
use crate::file_transfer::FileSession;
use crate::input::InputSession;
use crate::protocol::{Channel, ControlMsg, Frame, InputMsg};
use crate::transport::Transport;
use base64::Engine as _;
use clap::{Parser, Subcommand};
#[cfg(feature = "crypto")]
use getrandom::getrandom;
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
    },
    /// Connect to a host
    Connect {
        #[arg(long)]
        addr: SocketAddr,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        password: Option<String>,
    },
    /// Generate a new session identity
    Init,
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Host { listen, _unattended_password, file_root } => {
            host(listen, _unattended_password, file_root).await?;
        }
        Commands::Connect { addr, session_id, password } => {
            connect(addr, session_id, password).await?;
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
    let (screen_frame_tx, _screen_frame_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let recv_task = task::spawn(async move {
        let mut t = t_recv.lock().await;
        loop {
            match t.recv_frame().await {
                Ok(Some(Ok(frame))) => {
                    let mut c = cipher_recv.lock().await;
                    match c.decrypt(&frame.payload) {
                        Ok(plain) => {
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

    // Drop sender when receiver ends
    let _ = recv_task.await;
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

    let mut cipher = SessionCipher::new(&key_material);

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

    // --- Interactive loop: read stdin for input commands, print screen events ---
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
                send_input(&transport, &mut cipher, &next_seq, msg).await?;
            }
            "c" if parts.len() == 3 => {
                let x: i32 = parts[1].parse().unwrap_or(0);
                let y: i32 = parts[2].parse().unwrap_or(0);
                send_input(&transport, &mut cipher, &next_seq, InputMsg::MousePress { button: 1, x, y }).await?;
                send_input(&transport, &mut cipher, &next_seq, InputMsg::MouseRelease { button: 1, x, y }).await?;
            }
            "k" if parts.len() == 2 => {
                let code: u16 = parts[1].parse().unwrap_or(0);
                send_input(&transport, &mut cipher, &next_seq, InputMsg::KeyPress { code }).await?;
                send_input(&transport, &mut cipher, &next_seq, InputMsg::KeyRelease { code }).await?;
            }
            "t" if parts.len() >= 2 => {
                let text = parts[1..].join(" ");
                send_input(&transport, &mut cipher, &next_seq, InputMsg::KeyType { text }).await?;
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
    let bye_enc = cipher.encrypt(&bye_plain);
    let bye_frame = Frame { channel: Channel::Control, seq: next_seq.fetch_add(1, Ordering::Relaxed), payload: bye_enc };
    let mut t = transport.lock().await;
    let _ = t.send_frame(&bye_frame).await;

    Ok(())
}

async fn send_input(
    transport: &Arc<Mutex<Transport>>,
    cipher: &mut SessionCipher,
    next_seq: &Arc<AtomicU64>,
    msg: InputMsg,
) -> Result<(), Box<dyn std::error::Error>> {
    let plain = bincode::serialize(&msg)?;
    let enc = cipher.encrypt(&plain);
    let seq = next_seq.fetch_add(1, Ordering::Relaxed);
    let frame = Frame { channel: Channel::Input, seq, payload: enc };
    let mut t = transport.lock().await;
    t.send_frame(&frame).await?;
    Ok(())
}
