//! Loopback integration tests use real TCP, framing, PAKE, encryption and workers.
//! Mock desktop backends avoid screen recording and input permission prompts.
use super::*;
use crate::input::{InputError, InputInjector};
use crate::protocol::MonitorInfo as WireMonitor;
use crate::screen::{ScreenCapture, ScreenError};
use tokio::time::timeout;

struct MockScreen;
impl ScreenCapture for MockScreen {
    fn monitors(&self) -> Result<Vec<WireMonitor>, ScreenError> {
        Ok(vec![WireMonitor {
            index: 0,
            x: 0,
            y: 0,
            width: 2,
            height: 1,
            primary: true,
        }])
    }
    fn dimensions(&self, _: usize) -> Result<(u32, u32), ScreenError> {
        Ok((2, 1))
    }
    fn capture(&mut self, _: usize) -> Result<Vec<u8>, ScreenError> {
        Ok(vec![42; 8])
    }
}
struct MockInput(mpsc::Sender<InputMsg>);
impl InputInjector for MockInput {
    fn inject(&self, msg: &InputMsg) -> Result<(), InputError> {
        self.0
            .try_send(msg.clone())
            .map_err(|e| InputError::Injection(e.to_string()))
    }
}
fn config() -> SessionConfig {
    SessionConfig {
        password: Some("test password".into()),
        enable_input: false,
        enable_capture: false,
        enable_chat: true,
        ..SessionConfig::default()
    }
}
async fn socket_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (client, host) = tokio::join!(
        TcpStream::connect(listener.local_addr().unwrap()),
        listener.accept()
    );
    (host.unwrap().0, client.unwrap())
}
async fn pair() -> (SessionHandle, SessionHandle, mpsc::Receiver<InputMsg>) {
    let (host_socket, client_socket) = socket_pair().await;
    let (input_tx, input_rx) = mpsc::channel(16);
    let mut host_config = config();
    host_config.enable_input = true;
    let (host, client) = tokio::join!(
        SessionManager::handle_peer(
            host_socket,
            host_config,
            true,
            Some(ScreenSession::new(Box::new(MockScreen)).unwrap()),
            Some(InputSession::new(Box::new(MockInput(input_tx))))
        ),
        SessionManager::handle_peer(client_socket, config(), false, None, None)
    );
    (host.unwrap(), client.unwrap(), input_rx)
}
async fn next_matching(
    events: &mut SessionEvents,
    predicate: impl Fn(&SessionEvent) -> bool,
) -> SessionEvent {
    timeout(Duration::from_secs(3), async {
        loop {
            let event = events.recv().await.unwrap();
            if predicate(&event) {
                return event;
            }
        }
    })
    .await
    .expect("event timed out")
}
#[tokio::test]
async fn direct_session_frames_input_chat_and_disconnect() {
    let (host, client, mut input) = timeout(Duration::from_secs(3), pair()).await.unwrap();
    assert_eq!(host.id(), client.id());
    assert_eq!(host.state(), SessionState::Connected);
    let mut events = client.event_stream();
    let event = next_matching(&mut events, |e| {
        matches!(e, SessionEvent::ScreenFrame { .. })
    })
    .await;
    assert!(
        matches!(event, SessionEvent::ScreenFrame { width: 2, height: 1, data, .. } if data == vec![42; 8])
    );
    client
        .send_command(SessionCommand::MouseMove { x: 12, y: 34 })
        .await
        .unwrap();
    client
        .send_command(SessionCommand::MouseClick { button: 1 })
        .await
        .unwrap();
    client
        .send_command(SessionCommand::KeyPress { code: 65 })
        .await
        .unwrap();
    client
        .send_command(SessionCommand::KeyRelease { code: 65 })
        .await
        .unwrap();
    client
        .send_command(SessionCommand::KeyType {
            text: "hello".into(),
        })
        .await
        .unwrap();
    for index in 0..5 {
        let msg = timeout(Duration::from_secs(3), input.recv())
            .await
            .unwrap()
            .unwrap();
        match (index, msg) {
            (0, InputMsg::MouseMove { x: 12, y: 34 })
            | (1, InputMsg::MouseClick { button: 1 })
            | (2, InputMsg::KeyPress { code: 65 })
            | (3, InputMsg::KeyRelease { code: 65 }) => {}
            (4, InputMsg::KeyType { text }) if text == "hello" => {}
            other => panic!("unexpected input: {other:?}"),
        }
    }
    let mut host_events = host.event_stream();
    client
        .send_command(SessionCommand::ChatMessage {
            text: "ping".into(),
        })
        .await
        .unwrap();
    next_matching(
        &mut host_events,
        |e| matches!(e, SessionEvent::ChatMessage { text, .. } if text == "ping"),
    )
    .await;
    host.send_command(SessionCommand::ChatMessage {
        text: "pong".into(),
    })
    .await
    .unwrap();
    next_matching(
        &mut events,
        |e| matches!(e, SessionEvent::ChatMessage { text, .. } if text == "pong"),
    )
    .await;
    timeout(Duration::from_secs(3), client.disconnect())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(client.state(), SessionState::Disconnected);
    next_matching(&mut host_events, |e| {
        matches!(e, SessionEvent::Disconnected { .. })
    })
    .await;
    host.disconnect().await.unwrap();
    assert!(client
        .send_command(SessionCommand::KeyPress { code: 65 })
        .await
        .is_err());
    assert!(
        input.recv().await.is_none(),
        "input worker still owns backend after disconnect"
    );
}

#[tokio::test]
async fn last_handle_drop_cleans_up_workers() {
    let (host, client, mut input) = pair().await;
    let mut events = client.event_stream();
    let clone = host.clone();
    drop(host);
    assert_eq!(clone.state(), SessionState::Connected);
    drop(clone);
    next_matching(&mut events, |e| {
        matches!(e, SessionEvent::Disconnected { .. })
    })
    .await;
    assert!(timeout(Duration::from_secs(3), input.recv())
        .await
        .unwrap()
        .is_none());
    client.disconnect().await.unwrap();
}

#[tokio::test]
async fn queues_are_bounded_and_slow_event_consumers_report_lag() {
    let (host, client, _) = pair().await;
    assert_eq!(client.inner.command_tx.max_capacity(), 64);
    let mut events = client.event_stream();
    assert!(matches!(
        events.recv().await,
        Ok(SessionEvent::Connected { .. })
    ));
    for _ in 0..100 {
        let _ = client.inner.event_tx.send(SessionEvent::Error {
            message: "overflow".into(),
        });
    }
    assert!(matches!(
        events.recv().await,
        Err(broadcast::error::RecvError::Lagged(_))
    ));
    client.disconnect().await.unwrap();
    host.disconnect().await.unwrap();
}

#[tokio::test]
async fn wrong_password_cannot_create_session_handle() {
    let (host_socket, client_socket) = socket_pair().await;
    let mut wrong = config();
    wrong.password = Some("wrong password".into());
    let (host, client) = timeout(Duration::from_secs(3), async {
        tokio::join!(
            SessionManager::handle_peer(host_socket, config(), true, None, None),
            SessionManager::handle_peer(client_socket, wrong, false, None, None)
        )
    })
    .await
    .unwrap();
    assert!(matches!(host, Err(CoreError::Handshake(_))));
    assert!(matches!(client, Err(CoreError::Handshake(_))));
}

#[tokio::test]
async fn expected_session_id_mismatch_is_rejected() {
    let (host_socket, client_socket) = socket_pair().await;
    let mut mismatch = config();
    mismatch.session_id = Some(random_session_id());
    let (host, client) = timeout(Duration::from_secs(3), async {
        tokio::join!(
            SessionManager::handle_peer(host_socket, config(), true, None, None),
            SessionManager::handle_peer(client_socket, mismatch, false, None, None)
        )
    })
    .await
    .unwrap();
    assert!(host.is_err());
    assert!(
        matches!(client, Err(CoreError::Handshake(message)) if message.contains("session id mismatch"))
    );
}

#[tokio::test]
async fn successful_handshake_has_matching_directional_keys() {
    let (host, client) = socket_pair().await;
    let host = async {
        let mut transport = Transport::new(host);
        let (sid, mut cipher) = host_spake2_handshake(&mut transport, Some("password"), None)
            .await
            .unwrap();
        confirm_session(&mut transport, &mut cipher, true)
            .await
            .unwrap();
        (sid, cipher)
    };
    let client = async {
        let mut transport = Transport::new(client);
        let (sid, mut cipher) = client_spake2_handshake(&mut transport, None, Some("password"))
            .await
            .unwrap();
        confirm_session(&mut transport, &mut cipher, false)
            .await
            .unwrap();
        (sid, cipher)
    };
    let ((host_id, mut host), (client_id, mut client)) =
        timeout(Duration::from_secs(3), async { tokio::join!(host, client) })
            .await
            .unwrap();
    assert_eq!(host_id, client_id);
    let frame = encrypted_frame(&mut host, 2, Channel::Chat, &"host to client").unwrap();
    assert_eq!(
        bincode::deserialize::<String>(&decrypt_frame(&mut client, &frame, &mut 1).unwrap())
            .unwrap(),
        "host to client"
    );
    let frame = encrypted_frame(&mut client, 2, Channel::Chat, &"client to host").unwrap();
    assert_eq!(
        bincode::deserialize::<String>(&decrypt_frame(&mut host, &frame, &mut 1).unwrap()).unwrap(),
        "client to host"
    );
}

#[tokio::test]
async fn protocol_and_malformed_spake_messages_are_rejected() {
    for (version, message) in [
        (PROTOCOL_VERSION - 1, vec![1; 33]),
        (PROTOCOL_VERSION, vec![0]),
    ] {
        let (host, client) = socket_pair().await;
        let fake = async {
            Transport::new(host)
                .send_frame(&Frame {
                    channel: Channel::Control,
                    seq: 0,
                    payload: bincode::serialize(&ControlMsg::Hello {
                        version: 1,
                        protocol_version: version,
                        session_id: random_session_id(),
                        spake2_msg_1: message,
                    })
                    .unwrap(),
                })
                .await
                .unwrap();
        };
        let check = async {
            client_spake2_handshake(&mut Transport::new(client), None, Some("password")).await
        };
        let (_, result) = timeout(Duration::from_secs(3), async { tokio::join!(fake, check) })
            .await
            .unwrap();
        assert!(result.is_err());
    }
}

#[test]
fn transcript_salt_and_identities_are_bound_to_keys() {
    let sid = random_session_id();
    let material = handshake_material(b"shared", &sid, b"hello", b"ack").unwrap();
    assert_ne!(
        material,
        handshake_material(b"shared", &random_session_id(), b"hello", b"ack").unwrap()
    );
    assert_ne!(
        material,
        handshake_material(b"shared", &sid, b"changed hello", b"ack").unwrap()
    );
    assert_ne!(
        material,
        handshake_material(b"shared", &sid, b"hello", b"changed ack").unwrap()
    );
    let password = Password::new(b"password");
    let (host, host_msg) = Spake2::<Ed25519Group>::start_b(
        &password,
        &Identity::new(CLIENT_IDENTITY),
        &Identity::new(SERVER_IDENTITY),
    );
    let (client, client_msg) = Spake2::<Ed25519Group>::start_a(
        &password,
        &Identity::new(b"wrong-client"),
        &Identity::new(SERVER_IDENTITY),
    );
    assert_ne!(
        host.finish(&client_msg).unwrap(),
        client.finish(&host_msg).unwrap()
    );
}

#[test]
fn replay_and_header_tampering_are_rejected() {
    let material = [27; 128];
    let mut host = SessionCipher::for_role(&material, true);
    let mut client = SessionCipher::for_role(&material, false);
    let frame = encrypted_frame(&mut host, 2, Channel::Chat, &"message").unwrap();
    let mut seq = 1;
    decrypt_frame(&mut client, &frame, &mut seq).unwrap();
    assert!(decrypt_frame(&mut client, &frame, &mut seq).is_err());
    let mut tampered = frame.clone();
    tampered.channel = Channel::Input;
    assert!(decrypt_frame(&mut client, &tampered, &mut 1).is_err());
    let mut tampered = frame.clone();
    tampered.seq = 3;
    assert!(decrypt_frame(&mut client, &tampered, &mut 1).is_err());
}

#[tokio::test]
async fn relay_configuration_routes_actual_session_and_cancels_bridge() {
    let relay = SessionManager::start_relay("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let id = random_session_id();
    let mut host_config = config();
    host_config.relay_addr = Some(relay.local_addr());
    host_config.session_id = Some(id.clone());
    let mut client_config = host_config.clone();
    // A bogus direct address proves relay_addr takes precedence.
    client_config.connect_addr = Some("127.0.0.1:1".parse().unwrap());
    let host = SessionManager::start_host(host_config);
    let client = async {
        // Registration is processed asynchronously by the existing relay protocol.
        tokio::time::sleep(Duration::from_millis(100)).await;
        SessionManager::start_client(client_config).await
    };
    // Use public host/client API with input disabled; avoid real capture via test backend.
    let (host, client) = timeout(Duration::from_secs(3), async { tokio::join!(host, client) })
        .await
        .unwrap();
    let host = host.unwrap();
    let client = client.unwrap();
    assert_eq!(host.id(), id);
    assert_eq!(client.id(), id);
    client.disconnect().await.unwrap();
    host.disconnect().await.unwrap();
    relay.stop().await;
    assert!(TcpStream::connect(relay.local_addr()).await.is_err());
}

#[tokio::test]
async fn altered_hello_transcript_fails_mutual_confirmation() {
    let (host_socket, proxy_host) = socket_pair().await;
    let (proxy_client, client_socket) = socket_pair().await;
    let proxy = async {
        let mut host = Transport::new(proxy_host);
        let mut client = Transport::new(proxy_client);
        let mut hello = host.recv_frame().await.unwrap().unwrap().unwrap();
        let mut msg: ControlMsg = bincode::deserialize(&hello.payload).unwrap();
        if let ControlMsg::Hello { session_id, .. } = &mut msg {
            *session_id = random_session_id();
        }
        hello.payload = bincode::serialize(&msg).unwrap();
        client.send_frame(&hello).await.unwrap();
        let ack = client.recv_frame().await.unwrap().unwrap().unwrap();
        host.send_frame(&ack).await.unwrap();
        let confirmation = host.recv_frame().await.unwrap().unwrap().unwrap();
        client.send_frame(&confirmation).await.unwrap();
    };
    let (host, client, ()) = timeout(Duration::from_secs(3), async {
        tokio::join!(
            SessionManager::handle_peer(host_socket, config(), true, None, None),
            SessionManager::handle_peer(client_socket, config(), false, None, None),
            proxy
        )
    })
    .await
    .unwrap();
    assert!(host.is_err());
    assert!(client.is_err());
}

#[tokio::test]
async fn transport_accepts_screen_sized_frames_larger_than_four_mib() {
    let (host, client) = socket_pair().await;
    let bytes = vec![17; 5 * 1024 * 1024];
    let expected = bytes.clone();
    let send = async {
        Transport::new(host)
            .send_frame(&Frame {
                channel: Channel::Screen,
                seq: 1,
                payload: bytes,
            })
            .await
            .unwrap();
    };
    let receive = async {
        let frame = Transport::new(client)
            .recv_frame()
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(frame.payload, expected);
    };
    timeout(Duration::from_secs(3), async {
        tokio::join!(send, receive);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn disabled_input_does_not_call_host_backend() {
    let (host_socket, client_socket) = socket_pair().await;
    let (input_tx, mut input_rx) = mpsc::channel(1);
    let (host, client) = tokio::join!(
        SessionManager::handle_peer(
            host_socket,
            config(),
            true,
            None,
            Some(InputSession::new(Box::new(MockInput(input_tx))))
        ),
        SessionManager::handle_peer(client_socket, config(), false, None, None)
    );
    let host = host.unwrap();
    let client = client.unwrap();
    client
        .send_command(SessionCommand::KeyType {
            text: "must not type".into(),
        })
        .await
        .unwrap();
    let mut events = host.event_stream();
    client
        .send_command(SessionCommand::ChatMessage {
            text: "barrier".into(),
        })
        .await
        .unwrap();
    next_matching(
        &mut events,
        |e| matches!(e, SessionEvent::ChatMessage { text, .. } if text == "barrier"),
    )
    .await;
    assert!(input_rx.try_recv().is_err());
    client.disconnect().await.unwrap();
    host.disconnect().await.unwrap();
}

#[tokio::test]
async fn empty_password_is_rejected_before_unattended_connection() {
    let (host, _) = socket_pair().await;
    let mut config = config();
    config.password = Some(" ".into());
    let result = SessionManager::handle_peer(host, config, true, None, None).await;
    assert!(
        matches!(result, Err(CoreError::Session(message)) if message.contains("non-empty password"))
    );
}

#[tokio::test]
async fn denied_input_returns_explicit_permission_event() {
    let (host_socket, client_socket) = socket_pair().await;
    let (host, client) = tokio::join!(
        SessionManager::handle_peer(host_socket, config(), true, None, None),
        SessionManager::handle_peer(client_socket, config(), false, None, None)
    );
    let host = host.unwrap();
    let client = client.unwrap();
    let mut events = client.event_stream();
    client
        .send_command(SessionCommand::KeyPress { code: 65 })
        .await
        .unwrap();
    next_matching(&mut events, |event| {
        matches!(
            event,
            SessionEvent::PermissionDenied {
                channel: Channel::Input,
                ..
            }
        )
    })
    .await;
    client.disconnect().await.unwrap();
    host.disconnect().await.unwrap();
}

#[tokio::test]
async fn authorized_file_requests_are_dispatched_and_disabled_requests_are_denied() {
    let root =
        std::env::temp_dir().join(format!("easycontrol-session-file-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (host_socket, client_socket) = socket_pair().await;
    let mut host_config = config();
    host_config.enable_file_transfer = true;
    host_config.file_root = Some(root.clone());
    let (host, client) = tokio::join!(
        SessionManager::handle_peer(host_socket, host_config, true, None, None),
        SessionManager::handle_peer(client_socket, config(), false, None, None)
    );
    let host = host.unwrap();
    let client = client.unwrap();
    let mut events = client.event_stream();
    client
        .send_command(SessionCommand::FileWrite {
            path: "safe.txt".into(),
            offset: 0,
            data: b"safe".to_vec(),
        })
        .await
        .unwrap();
    next_matching(&mut events, |event| {
        matches!(
            event,
            SessionEvent::FileWritten {
                bytes_written: 4,
                ..
            }
        )
    })
    .await;
    client
        .send_command(SessionCommand::FileRead {
            path: "safe.txt".into(),
            offset: 0,
            length: 16,
        })
        .await
        .unwrap();
    next_matching(
        &mut events,
        |event| matches!(event, SessionEvent::FileData { data, .. } if data == b"safe"),
    )
    .await;
    client
        .send_command(SessionCommand::FileRead {
            path: "../escape".into(),
            offset: 0,
            length: 1,
        })
        .await
        .unwrap();
    next_matching(&mut events, |event| {
        matches!(
            event,
            SessionEvent::PermissionDenied {
                channel: Channel::File,
                ..
            }
        )
    })
    .await;
    client.disconnect().await.unwrap();
    host.disconnect().await.unwrap();
    let _ = std::fs::remove_dir_all(root);
}
