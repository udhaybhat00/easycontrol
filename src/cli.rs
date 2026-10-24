use crate::relay::RelayServer;
use crate::session::{SessionCommand, SessionConfig, SessionEvent, SessionHandle, SessionManager};
use base64::Engine as _;
use clap::{Parser, Subcommand};
use getrandom::getrandom;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::info;

#[derive(Parser)]
#[command(name = "easycontrol")]
#[command(about = "Free, fast remote desktop tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Host {
        #[arg(long, default_value = "0.0.0.0:5900")]
        listen: SocketAddr,
        #[arg(long)]
        password: Option<String>,
        #[arg(long, default_value = "./easycontrol_files")]
        file_root: String,
        #[arg(long)]
        relay: Option<String>,
    },
    Connect {
        #[arg(long, required_unless_present = "relay")]
        addr: Option<SocketAddr>,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        relay: Option<String>,
    },
    Init,
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
            password,
            file_root,
            relay,
        } => {
            if let Some(relay_addr) = relay {
                host_via_relay(&relay_addr, password, file_root).await?;
            } else {
                host(listen, password, file_root).await?;
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
                connect(
                    addr.ok_or("--addr required for direct mode")?,
                    session_id,
                    password,
                )
                .await?;
            }
        }
        Commands::Init => {
            let mut secret = [0u8; 32];
            getrandom(&mut secret).expect("failed to get random bytes");
            println!(
                "{}",
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret)
            );
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
    password: Option<String>,
    file_root: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = SessionConfig {
        listen_addr: Some(listen),
        password,
        file_root: Some(PathBuf::from(file_root)),
        ..SessionConfig::default()
    };

    let handle = SessionManager::start_host(config).await?;
    info!("Session started: {}", handle.id());

    run_client_loop(handle).await
}

async fn connect(
    addr: SocketAddr,
    session_id: Option<String>,
    password: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = SessionConfig {
        connect_addr: Some(addr),
        session_id,
        password,
        ..SessionConfig::default()
    };

    let handle = SessionManager::start_client(config).await?;
    info!("Connected to session: {}", handle.id());

    run_client_loop(handle).await
}

async fn host_via_relay(
    relay_addr: &str,
    password: Option<String>,
    file_root: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut identity = [0u8; 32];
    getrandom(&mut identity)?;
    let session_id = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(identity);
    println!("Relay session id: {}", session_id);
    let config = SessionConfig {
        listen_addr: None,
        connect_addr: None,
        relay_addr: Some(relay_addr.parse()?),
        session_id: Some(session_id),
        password,
        file_root: Some(PathBuf::from(file_root)),
        ..SessionConfig::default()
    };

    let handle = SessionManager::start_host(config).await?;
    info!("Host registered on relay, session id: {}", handle.id());

    run_client_loop(handle).await
}

async fn connect_via_relay(
    relay_addr: &str,
    session_id: Option<String>,
    password: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = SessionConfig {
        listen_addr: None,
        connect_addr: None,
        relay_addr: Some(relay_addr.parse()?),
        session_id,
        password,
        ..SessionConfig::default()
    };

    let handle = SessionManager::start_client(config).await?;
    info!("Connected via relay to session: {}", handle.id());

    run_client_loop(handle).await
}

async fn run_client_loop(handle: SessionHandle) -> Result<(), Box<dyn std::error::Error>> {
    let mut event_stream = handle.event_stream();

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

    loop {
        tokio::select! {
            result = lines.next_line() => {
                match result? {
                    Some(line) => {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.is_empty() {
                            print!("> ");
                            io::stdout().flush()?;
                            continue;
                        }

                        match parts[0] {
                            "q" | "quit" => {
                                handle.disconnect().await?;
                                break;
                            }
                            "m" if parts.len() == 3 => {
                                let x: i32 = parts[1].parse().unwrap_or(0);
                                let y: i32 = parts[2].parse().unwrap_or(0);
                                handle.send_command(SessionCommand::MouseMove { x, y }).await?;
                            }
                            "c" if parts.len() == 3 => {
                                let x: i32 = parts[1].parse().unwrap_or(0);
                                let y: i32 = parts[2].parse().unwrap_or(0);
                                handle.send_command(SessionCommand::MouseMove { x, y }).await?;
                                handle.send_command(SessionCommand::MouseClick { button: 1 }).await?;
                            }
                            "k" if parts.len() == 2 => {
                                let code: u16 = parts[1].parse().unwrap_or(0);
                                handle.send_command(SessionCommand::KeyPress { code }).await?;
                                handle.send_command(SessionCommand::KeyRelease { code }).await?;
                            }
                            "t" if parts.len() >= 2 => {
                                let text = parts[1..].join(" ");
                                handle.send_command(SessionCommand::KeyType { text }).await?;
                            }
                            "clipboard" => {
                                if parts.len() >= 2 && parts[1] == "get" {
                                    handle.send_command(SessionCommand::ClipboardGet).await?;
                                } else if parts.len() >= 3 && parts[1] == "set" {
                                    let text = parts[2..].join(" ");
                                    handle.send_command(SessionCommand::ClipboardSet { text }).await?;
                                } else {
                                    println!("usage: clipboard get | clipboard set <text>");
                                }
                            }
                            "chat" if parts.len() >= 2 => {
                                let text = parts[1..].join(" ");
                                handle.send_command(SessionCommand::ChatMessage { text }).await?;
                            }
                            "monitor" => {
                                if parts.len() >= 2 && parts[1] == "list" {
                                    handle.send_command(SessionCommand::MonitorSelect { index: 0 }).await?;
                                } else if parts.len() >= 3 && parts[1] == "select" {
                                    let index: usize = parts[2].parse().unwrap_or(0);
                                    handle.send_command(SessionCommand::MonitorSelect { index }).await?;
                                } else {
                                    println!("usage: monitor list | monitor select <index>");
                                }
                            }
                            _ => {
                                println!("unknown command");
                            }
                        }
                    }
                    None => break,
                }
                print!("> ");
                io::stdout().flush()?;
            }
            result = event_stream.recv() => {
                match result {
                    Ok(event) => match event {
                        SessionEvent::Connected { session_id } => {
                            println!("\nConnected to session: {}", session_id);
                            print!("> ");
                            io::stdout().flush()?;
                        }
                        SessionEvent::Disconnected { reason } => {
                            println!("\nDisconnected: {}", reason);
                            break;
                        }
                        SessionEvent::ScreenFrame { width, height, data, .. } => {
                            let _ = (width, height, data);
                        }
                        SessionEvent::ClipboardData { data, mime } => {
                            if let Ok(text) = String::from_utf8(data.clone()) {
                                println!("\nClipboard ({}): {}", mime, text);
                            }
                            print!("> ");
                            io::stdout().flush()?;
                        }
                        SessionEvent::FileList { entries } => println!("\nReceived {} file entries", entries.len()),
                        SessionEvent::FileData { path, offset, data } => println!("\nReceived {} bytes from {} at {}", data.len(), path, offset),
                        SessionEvent::FileWritten { path, offset, bytes_written } => println!("\nWrote {} bytes to {} at {}", bytes_written, path, offset),
                        SessionEvent::PermissionDenied { channel, reason } => println!("\nPermission denied for {:?}: {}", channel, reason),
                        SessionEvent::ChatMessage { from, text } => {
                            println!("\n[chat from {}] {}", from, text);
                            print!("> ");
                            io::stdout().flush()?;
                        }
                        SessionEvent::MonitorList { monitors } => {
                            println!("\nMonitors:");
                            for m in &monitors {
                                println!(
                                    "  Monitor {}: {}x{} at ({}, {}){}",
                                    m.index, m.width, m.height, m.x, m.y,
                                    if m.primary { " (primary)" } else { "" }
                                );
                            }
                            print!("> ");
                            io::stdout().flush()?;
                        }
                        SessionEvent::Error { message } => {
                            println!("\nError: {}", message);
                            print!("> ");
                            io::stdout().flush()?;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    handle.disconnect().await?;
    Ok(())
}
