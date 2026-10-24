//! Public SessionHandle API, using real loopback sockets and no desktop access.
use easycontrol::{SessionCommand, SessionConfig, SessionEvent, SessionManager, SessionState};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;

#[tokio::test]
async fn public_direct_connection_and_bidirectional_events() {
    timeout(Duration::from_secs(3), async {
        let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = reservation.local_addr().unwrap();
        let host_config = SessionConfig {
            listen_addr: Some(address),
            password: Some("loopback password".into()),
            enable_input: false,
            enable_capture: false,
            ..SessionConfig::default()
        };
        let client_config = SessionConfig {
            connect_addr: Some(address),
            password: Some("loopback password".into()),
            enable_input: false,
            enable_capture: false,
            ..SessionConfig::default()
        };
        drop(reservation);
        // join polls host first: its listener binds before the connect future runs.
        let (host, client) = tokio::join!(
            SessionManager::start_host(host_config),
            SessionManager::start_client(client_config)
        );
        let host = host.unwrap();
        let client = client.unwrap();
        assert_eq!(host.id(), client.id());
        let mut host_events = host.event_stream();
        let mut client_events = client.event_stream();
        assert!(matches!(
            host_events.recv().await,
            Ok(SessionEvent::Connected { .. })
        ));
        assert!(matches!(
            client_events.recv().await,
            Ok(SessionEvent::Connected { .. })
        ));
        client
            .send_command(SessionCommand::ChatMessage {
                text: "client to host".into(),
            })
            .await
            .unwrap();
        loop {
            if let SessionEvent::ChatMessage { text, .. } = host_events.recv().await.unwrap() {
                assert_eq!(text, "client to host");
                break;
            }
        }
        host.send_command(SessionCommand::ChatMessage {
            text: "host to client".into(),
        })
        .await
        .unwrap();
        loop {
            if let SessionEvent::ChatMessage { text, .. } = client_events.recv().await.unwrap() {
                assert_eq!(text, "host to client");
                break;
            }
        }
        client.disconnect().await.unwrap();
        loop {
            if matches!(
                host_events.recv().await.unwrap(),
                SessionEvent::Disconnected { .. }
            ) {
                break;
            }
        }
        host.disconnect().await.unwrap();
        assert_eq!(host.state(), SessionState::Disconnected);
        assert_eq!(client.state(), SessionState::Disconnected);
        client.disconnect().await.unwrap(); // idempotent
    })
    .await
    .expect("public session test timed out");
}
