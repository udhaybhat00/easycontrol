use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio::time;

const REGISTRATION_TIMEOUT_SECS: u64 = 60;
const REGISTRATION_MAX_BYTES: usize = 4096;
const REGISTRATION_DEADLINE_SECS: u64 = 5;

type HmacSha256 = Hmac<Sha256>;
struct PendingHost {
    stream: TcpStream,
    token: [u8; 32],
}

pub struct RelayServer {
    listen_addr: SocketAddr,
    pending_hosts: Arc<Mutex<HashMap<String, PendingHost>>>,
}

impl RelayServer {
    pub fn new(listen_addr: SocketAddr) -> Self {
        Self {
            listen_addr,
            pending_hosts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn run(self) -> io::Result<()> {
        let listener = TcpListener::bind(self.listen_addr).await?;
        self.run_listener(listener).await
    }

    pub(crate) async fn run_listener(self, listener: TcpListener) -> io::Result<()> {
        tracing::info!("relay server listening on {}", listener.local_addr()?);
        let pending_hosts = self.pending_hosts;
        let mut peers = JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let (stream, peer) = accepted?;
                    let pending_hosts = pending_hosts.clone();
                    peers.spawn(async move {
                        if let Err(e) = handle_relay_peer(stream, peer, pending_hosts).await {
                            tracing::warn!("relay peer error from {}: {}", peer, e);
                        }
                    });
                }
                _ = peers.join_next(), if !peers.is_empty() => {}
            }
        }
    }
}

async fn handle_relay_peer(
    mut stream: TcpStream,
    peer: SocketAddr,
    pending_hosts: Arc<Mutex<HashMap<String, PendingHost>>>,
) -> io::Result<()> {
    // Read registration: 4-byte big-endian length + JSON payload
    let mut len_buf = [0u8; 4];
    time::timeout(
        Duration::from_secs(REGISTRATION_DEADLINE_SECS),
        stream.read_exact(&mut len_buf),
    )
    .await??;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > REGISTRATION_MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "relay registration too large",
        ));
    }
    let mut payload = vec![0u8; len];
    time::timeout(
        Duration::from_secs(REGISTRATION_DEADLINE_SECS),
        stream.read_exact(&mut payload),
    )
    .await??;

    let reg: Registration = serde_json::from_slice(&payload)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    match reg.role {
        Role::Host => {
            let session_id = reg.session_id.clone();
            tracing::info!("host registered session {} from {}", session_id, peer);
            let token = reg.token;
            let mut sessions = pending_hosts.lock().await;
            if sessions.contains_key(&session_id) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "relay session already registered",
                ));
            }
            sessions.insert(session_id.clone(), PendingHost { stream, token });
            drop(sessions);
            // Keep this task alive so the host stream stays registered in pending_hosts.
            // The entry is removed when a client connects and bridges the connection.
            // Timeout after REGISTRATION_TIMEOUT_SECS to prevent stale registrations.
            match time::timeout(
                Duration::from_secs(REGISTRATION_TIMEOUT_SECS),
                std::future::pending::<()>(),
            )
            .await
            {
                Ok(_) => {}
                Err(_) => {
                    tracing::info!("host registration timed out for session {}", session_id);
                    pending_hosts.lock().await.remove(&session_id);
                }
            }
            Ok(()) // unreachable
        }
        Role::Client { target_session } => {
            let host = {
                let mut s = pending_hosts.lock().await;
                s.remove(&target_session)
            };

            match host {
                Some(PendingHost {
                    stream: mut host_stream,
                    token,
                }) if token == reg.token => {
                    tracing::info!(
                        "client connected to session {} from {}",
                        target_session,
                        peer
                    );
                    // Bridge client and host bidirectionally
                    let _ = tokio::io::copy_bidirectional(&mut stream, &mut host_stream).await?;
                    Ok(())
                }
                Some(_) => Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "relay registration token mismatch",
                )),
                None => Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("session {} not found", target_session),
                )),
            }
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
struct Registration {
    role: Role,
    session_id: String,
    token: [u8; 32],
}

#[derive(serde::Deserialize, serde::Serialize)]
enum Role {
    Host,
    Client { target_session: String },
}

/// Connect to the relay as a host: register session_id, return the connected stream.
pub async fn connect_as_host(
    relay_addr: &str,
    session_id: &str,
    password: &str,
) -> io::Result<TcpStream> {
    let mut stream = TcpStream::connect(relay_addr).await?;
    let reg = Registration {
        role: Role::Host,
        session_id: session_id.to_string(),
        token: registration_token(session_id, password)?,
    };
    let payload = serde_json::to_vec(&reg).unwrap();
    let framed = encode_frame(&payload);
    stream.write_all(&framed).await?;
    Ok(stream)
}

/// Connect to the relay as a client: target session_id, return the connected stream.
pub async fn connect_as_client(
    relay_addr: &str,
    target_session: &str,
    password: &str,
) -> io::Result<TcpStream> {
    let mut stream = TcpStream::connect(relay_addr).await?;
    let reg = Registration {
        role: Role::Client {
            target_session: target_session.to_string(),
        },
        session_id: String::new(),
        token: registration_token(target_session, password)?,
    };
    let payload = serde_json::to_vec(&reg).unwrap();
    let framed = encode_frame(&payload);
    stream.write_all(&framed).await?;
    Ok(stream)
}

fn registration_token(session_id: &str, password: &str) -> io::Result<[u8; 32]> {
    let mut mac = HmacSha256::new_from_slice(password.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid relay secret"))?;
    mac.update(b"easycontrol-v5-relay-registration");
    mac.update(session_id.as_bytes());
    Ok(mac.finalize().into_bytes().into())
}

fn encode_frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_tokens_prevent_session_id_only_hijacking() {
        let id = "session-id";
        assert_ne!(
            registration_token(id, "host secret").unwrap(),
            registration_token(id, "attacker secret").unwrap()
        );
        assert_ne!(
            registration_token(id, "host secret").unwrap(),
            registration_token("other-id", "host secret").unwrap()
        );
    }
}
