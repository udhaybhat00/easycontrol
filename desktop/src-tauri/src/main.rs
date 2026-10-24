use easycontrol::{SessionConfig, SessionEvent, SessionHandle, SessionManager};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, RwLock};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Permissions {
    input: bool,
    clipboard: bool,
    file_transfer: bool,
    chat: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    state: String,
    detail: String,
    session_id: Option<String>,
}

#[derive(Clone, Serialize)]
struct FrontendEvent {
    kind: String,
    detail: Option<String>,
    session_id: Option<String>,
}

struct AppState {
    session: Mutex<Option<SessionHandle>>,
    host_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    permissions: RwLock<Permissions>,
    status: RwLock<Status>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            session: Mutex::new(None),
            host_task: Mutex::new(None),
            permissions: RwLock::new(Permissions {
                input: false,
                clipboard: false,
                file_transfer: false,
                chat: false,
            }),
            status: RwLock::new(Status {
                state: "idle".into(),
                detail: "Ready for a direct connection".into(),
                session_id: None,
            }),
        }
    }
}

fn config(password: String, permissions: Permissions) -> SessionConfig {
    SessionConfig {
        password: Some(password),
        enable_input: permissions.input,
        enable_clipboard: permissions.clipboard,
        enable_file_transfer: permissions.file_transfer,
        enable_chat: permissions.chat,
        ..SessionConfig::default()
    }
}

async fn set_status(state: &AppState, next: Status) {
    *state.status.write().await = next;
}

fn forward_events(app: AppHandle, handle: SessionHandle) {
    tauri::async_runtime::spawn(async move {
        let mut events = handle.event_stream();
        while let Ok(event) = events.recv().await {
            let payload = match event {
                SessionEvent::Connected { session_id } => FrontendEvent {
                    kind: "connected".into(),
                    detail: None,
                    session_id: Some(session_id),
                },
                SessionEvent::Disconnected { reason } => FrontendEvent {
                    kind: "disconnected".into(),
                    detail: Some(reason),
                    session_id: None,
                },
                SessionEvent::Error { message } => FrontendEvent {
                    kind: "error".into(),
                    detail: Some(message),
                    session_id: None,
                },
                SessionEvent::PermissionDenied { reason, .. } => FrontendEvent {
                    kind: "denied".into(),
                    detail: Some(reason),
                    session_id: None,
                },
                SessionEvent::MonitorList { .. } => FrontendEvent {
                    kind: "monitors".into(),
                    detail: Some("Monitor list updated".into()),
                    session_id: None,
                },
                SessionEvent::ScreenFrame { .. }
                | SessionEvent::ClipboardData { .. }
                | SessionEvent::ChatMessage { .. }
                | SessionEvent::FileList { .. }
                | SessionEvent::FileData { .. }
                | SessionEvent::FileWritten { .. } => continue,
            };
            let _ = app.emit("session-event", payload);
        }
    });
}

#[tauri::command]
async fn configure_permissions(
    state: State<'_, AppState>,
    permissions: Permissions,
) -> Result<(), String> {
    *state.permissions.write().await = permissions;
    Ok(())
}

#[tauri::command]
async fn connection_status(state: State<'_, AppState>) -> Result<Status, String> {
    Ok(state.status.read().await.clone())
}

#[tauri::command]
async fn connect(
    app: AppHandle,
    state: State<'_, AppState>,
    address: String,
    password: String,
    permissions: Permissions,
) -> Result<(), String> {
    let address: SocketAddr = address
        .parse()
        .map_err(|_| "Use a direct host:port address".to_string())?;
    if password.trim().is_empty() {
        return Err("A non-empty session password is required".into());
    }
    set_status(
        &state,
        Status {
            state: "connecting".into(),
            detail: "Authenticating connection".into(),
            session_id: None,
        },
    )
    .await;
    let handle = SessionManager::start_client(SessionConfig {
        connect_addr: Some(address),
        ..config(password, permissions)
    })
    .await
    .map_err(|error| error.to_string())?;
    let id = handle.id().to_string();
    *state.session.lock().await = Some(handle.clone());
    set_status(
        &state,
        Status {
            state: "connected".into(),
            detail: "Encrypted session active".into(),
            session_id: Some(id),
        },
    )
    .await;
    forward_events(app, handle);
    Ok(())
}

#[tauri::command]
async fn start_host(
    app: AppHandle,
    state: State<'_, AppState>,
    port: u16,
    password: String,
    permissions: Permissions,
) -> Result<(), String> {
    if password.trim().is_empty() {
        return Err("A non-empty session password is required".into());
    }
    if state.host_task.lock().await.is_some() {
        return Err("A host is already starting or active".into());
    }
    set_status(
        &state,
        Status {
            state: "hosting".into(),
            detail: format!("Waiting for a direct connection on port {port}"),
            session_id: None,
        },
    )
    .await;
    let app_clone = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let result = SessionManager::start_host(SessionConfig {
            listen_addr: Some(([0, 0, 0, 0], port).into()),
            ..config(password, permissions)
        })
        .await;
        match result {
            Ok(handle) => {
                let id = handle.id().to_string();
                let state_ref = app_clone.state::<AppState>();
                *state_ref.session.lock().await = Some(handle.clone());
                set_status(
                    &state_ref,
                    Status {
                        state: "connected".into(),
                        detail: "Encrypted session active".into(),
                        session_id: Some(id),
                    },
                )
                .await;
                forward_events(app_clone, handle);
            }
            Err(error) => {
                let state_ref = app_clone.state::<AppState>();
                set_status(
                    &state_ref,
                    Status {
                        state: "idle".into(),
                        detail: error.to_string(),
                        session_id: None,
                    },
                )
                .await;
                let _ = app_clone.emit(
                    "session-event",
                    FrontendEvent {
                        kind: "error".into(),
                        detail: Some(error.to_string()),
                        session_id: None,
                    },
                );
            }
        }
    });
    *state.host_task.lock().await = Some(task);
    Ok(())
}

#[tauri::command]
async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(task) = state.host_task.lock().await.take() {
        task.abort();
    }
    if let Some(handle) = state.session.lock().await.take() {
        handle
            .disconnect()
            .await
            .map_err(|error| error.to_string())?;
    }
    set_status(
        &state,
        Status {
            state: "idle".into(),
            detail: "Disconnected".into(),
            session_id: None,
        },
    )
    .await;
    Ok(())
}

#[tauri::command]
async fn available_monitors(_state: State<'_, AppState>) -> Result<Vec<MonitorInfo>, String> {
    Ok(Vec::new())
}

#[derive(Serialize)]
struct MonitorInfo {
    index: usize,
    width: u32,
    height: u32,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            start_host,
            connect,
            disconnect,
            connection_status,
            configure_permissions,
            available_monitors
        ])
        .run(tauri::generate_context!())
        .expect("failed to run EasyControl desktop");
}
