use easycontrol::{
    MonitorInfo as CoreMonitorInfo, SessionCommand, SessionConfig, SessionEvent, SessionHandle,
    SessionManager,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::process::Command;
use std::time::Instant;
use tauri::{ipc::Response, AppHandle, Emitter, Manager, State};
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
struct HostSessionStatus {
    active: bool,
    screen_sharing: bool,
    remote_input: bool,
    clipboard: bool,
    file_transfer: bool,
    peer: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    state: String,
    detail: String,
    session_id: Option<String>,
    host: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FrontendEvent {
    kind: String,
    detail: Option<String>,
    session_id: Option<String>,
}

#[derive(Clone)]
struct LatestFrame {
    sequence: u64,
    width: u32,
    height: u32,
    stride: u32,
    received_at: Instant,
    data: Vec<u8>,
}

#[derive(Default)]
struct FrameStore {
    latest: Option<LatestFrame>,
    next_sequence: u64,
    dropped: u64,
    delivered_sequence: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FrameStats {
    sequence: u64,
    width: u32,
    height: u32,
    dropped: u64,
    age_ms: u64,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum ViewerInput {
    MouseMove { x: i32, y: i32 },
    MouseDown { button: u8, x: i32, y: i32 },
    MouseUp { button: u8, x: i32, y: i32 },
    MouseWheel { dx: i32, dy: i32 },
    KeyDown { code: u16 },
    KeyUp { code: u16 },
}

struct AppState {
    operation: Mutex<()>,
    session: Mutex<Option<SessionHandle>>,
    host_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    permissions: RwLock<Permissions>,
    status: RwLock<Status>,
    frames: RwLock<FrameStore>,
    monitors: RwLock<Vec<CoreMonitorInfo>>,
    host: RwLock<bool>,
    sharing: RwLock<bool>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            operation: Mutex::new(()),
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
                host: false,
            }),
            frames: RwLock::new(FrameStore::default()),
            monitors: RwLock::new(Vec::new()),
            host: RwLock::new(false),
            sharing: RwLock::new(false),
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

async fn clear_viewer_state(state: &AppState) {
    *state.sharing.write().await = false;
    *state.frames.write().await = FrameStore::default();
    state.monitors.write().await.clear();
}

async fn store_frame(
    state: &AppState,
    width: u32,
    height: u32,
    stride: u32,
    format: String,
    data: Vec<u8>,
) {
    let expected = stride
        .checked_mul(height)
        .map(usize::try_from)
        .and_then(Result::ok);
    if width == 0
        || height == 0
        || data.len() > 64 * 1024 * 1024
        || format != "RGBA8"
        || stride < width.saturating_mul(4)
        || expected != Some(data.len())
    {
        tracing::warn!(
            width,
            height,
            stride,
            bytes = data.len(),
            "Rejected invalid screen frame metadata"
        );
        return;
    }
    tracing::debug!(
        width,
        height,
        stride,
        bytes = data.len(),
        "Accepted screen frame"
    );
    let mut frames = state.frames.write().await;
    if frames
        .latest
        .as_ref()
        .is_some_and(|frame| frame.sequence > frames.delivered_sequence)
    {
        frames.dropped = frames.dropped.saturating_add(1);
    }
    frames.next_sequence = frames.next_sequence.saturating_add(1);
    let sequence = frames.next_sequence;
    frames.latest = Some(LatestFrame {
        sequence,
        width,
        height,
        stride,
        received_at: Instant::now(),
        data,
    });
}

fn encode_frame(frame: &LatestFrame, dropped: u64) -> Vec<u8> {
    let mut body = Vec::with_capacity(32 + frame.data.len());
    body.extend_from_slice(b"ECF1");
    body.extend_from_slice(&frame.sequence.to_le_bytes());
    body.extend_from_slice(&frame.width.to_le_bytes());
    body.extend_from_slice(&frame.height.to_le_bytes());
    body.extend_from_slice(&frame.stride.to_le_bytes());
    body.extend_from_slice(&dropped.to_le_bytes());
    body.extend_from_slice(&frame.data);
    body
}

fn forward_events(app: AppHandle, handle: SessionHandle) {
    tauri::async_runtime::spawn(async move {
        let mut events = handle.event_stream();
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            let state = app.state::<AppState>();
            let _operation = state.operation.lock().await;
            if state.status.read().await.session_id.as_deref() != Some(handle.id()) {
                break;
            }
            let disconnected = matches!(event, SessionEvent::Disconnected { .. });
            let payload = match event {
                SessionEvent::Connected { session_id } => FrontendEvent {
                    kind: "connected".into(),
                    detail: None,
                    session_id: Some(session_id),
                },
                SessionEvent::Disconnected { reason } => {
                    clear_viewer_state(&state).await;
                    *state.session.lock().await = None;
                    *state.host.write().await = false;
                    set_status(
                        &state,
                        Status {
                            state: "idle".into(),
                            detail: reason.clone(),
                            session_id: None,
                            host: false,
                        },
                    )
                    .await;
                    FrontendEvent {
                        kind: "disconnected".into(),
                        detail: Some(reason),
                        session_id: None,
                    }
                }
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
                SessionEvent::MonitorList { monitors } => {
                    *state.monitors.write().await = monitors;
                    FrontendEvent {
                        kind: "monitors".into(),
                        detail: Some("Monitor list updated".into()),
                        session_id: None,
                    }
                }
                SessionEvent::ScreenFrame {
                    width,
                    height,
                    stride,
                    format,
                    data,
                } => {
                    store_frame(&state, width, height, stride, format, data).await;
                    continue;
                }
                SessionEvent::ScreenSharing { active } => {
                    *state.sharing.write().await = active;
                    continue;
                }
                SessionEvent::ClipboardData { .. }
                | SessionEvent::ChatMessage { .. }
                | SessionEvent::FileList { .. }
                | SessionEvent::FileData { .. }
                | SessionEvent::FileWritten { .. } => continue,
            };
            let _ = app.emit("session-event", payload);
            if disconnected {
                break;
            }
        }
    });
}

#[tauri::command]
async fn configure_permissions(
    state: State<'_, AppState>,
    permissions: Permissions,
) -> Result<(), String> {
    let _operation = state.operation.lock().await;
    let previous = state.permissions.read().await.clone();
    if (previous.clipboard != permissions.clipboard
        || previous.file_transfer != permissions.file_transfer
        || previous.chat != permissions.chat)
        && state.status.read().await.state != "idle"
    {
        return Err("Stop the session before changing clipboard, file or chat permissions".into());
    }
    if *state.host.read().await && previous.input != permissions.input {
        if let Some(handle) = state.session.lock().await.clone() {
            handle
                .set_remote_input_enabled(permissions.input)
                .await
                .map_err(|error| error.to_string())?;
        }
    }
    *state.permissions.write().await = permissions;
    Ok(())
}

#[tauri::command]
fn desktop_permissions() -> permissions::DesktopPermissions {
    permissions::status()
}

#[tauri::command]
fn request_screen_recording_permission() -> bool {
    permissions::request_screen_recording()
}

#[tauri::command]
fn open_privacy_settings(kind: String) -> Result<(), String> {
    permissions::open_settings(&kind)
}

#[tauri::command]
fn open_creator_profile(profile: String) -> Result<(), String> {
    let url = match profile.as_str() {
        "source" => "https://github.com/udhaybhat00/easycontrol",
        "issues" => "https://github.com/udhaybhat00/easycontrol/issues",
        "github" => "https://github.com/udhaybhat00",
        "linkedin" => "https://in.linkedin.com/in/udhaybhat",
        _ => return Err("Unknown creator profile".into()),
    };
    #[cfg(target_os = "macos")]
    Command::new("open")
        .arg(url)
        .spawn()
        .map_err(|error| format!("Could not open the browser: {error}"))?;
    #[cfg(target_os = "windows")]
    Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn()
        .map_err(|error| format!("Could not open the browser: {error}"))?;
    #[cfg(all(unix, not(target_os = "macos")))]
    Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map_err(|error| format!("Could not open the browser: {error}"))?;
    Ok(())
}

#[tauri::command]
async fn host_session_status(state: State<'_, AppState>) -> Result<HostSessionStatus, String> {
    let host = *state.host.read().await;
    let permissions = state.permissions.read().await.clone();
    let active = host && state.session.lock().await.is_some();
    Ok(HostSessionStatus {
        active,
        screen_sharing: active && *state.sharing.read().await,
        remote_input: host && permissions.input,
        clipboard: host && permissions.clipboard,
        file_transfer: host && permissions.file_transfer,
        peer: None,
    })
}

#[tauri::command]
async fn latest_frame(
    state: State<'_, AppState>,
    session_id: String,
    after_sequence: u64,
) -> Result<Response, String> {
    Ok(Response::new(
        frame_packet(&state, &session_id, after_sequence).await,
    ))
}

async fn frame_packet(state: &AppState, session_id: &str, after_sequence: u64) -> Vec<u8> {
    let _operation = state.operation.lock().await;
    let status = state.status.read().await;
    if status.state != "connected"
        || status.host
        || status.session_id.as_deref() != Some(session_id)
    {
        return Vec::new();
    }
    let mut frames = state.frames.write().await;
    let Some(frame) = frames
        .latest
        .as_ref()
        .filter(|frame| frame.sequence > after_sequence)
    else {
        return Vec::new();
    };
    let body = encode_frame(frame, frames.dropped);
    frames.delivered_sequence = frame.sequence;
    body
}

#[tauri::command]
async fn frame_stats(state: State<'_, AppState>) -> Result<Option<FrameStats>, String> {
    let frames = state.frames.read().await;
    Ok(frames.latest.as_ref().map(|frame| FrameStats {
        sequence: frame.sequence,
        width: frame.width,
        height: frame.height,
        dropped: frames.dropped,
        age_ms: frame
            .received_at
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
    }))
}

#[tauri::command]
async fn viewer_input(
    state: State<'_, AppState>,
    session_id: String,
    input: ViewerInput,
) -> Result<(), String> {
    let _operation = state.operation.lock().await;
    let status = state.status.read().await;
    if status.state != "connected"
        || status.host
        || status.session_id.as_deref() != Some(&session_id)
    {
        return Err("No authenticated session is active".into());
    }
    if !state.permissions.read().await.input {
        return Err("Remote input is disabled locally; the host must also permit it".into());
    }
    let command = match input {
        ViewerInput::MouseMove { x, y } => SessionCommand::MouseMove { x, y },
        ViewerInput::MouseDown { button, x, y } => SessionCommand::MousePress { button, x, y },
        ViewerInput::MouseUp { button, x, y } => SessionCommand::MouseRelease { button, x, y },
        ViewerInput::MouseWheel { dx, dy } => SessionCommand::MouseScroll { dx, dy },
        ViewerInput::KeyDown { code } => SessionCommand::KeyPress { code },
        ViewerInput::KeyUp { code } => SessionCommand::KeyRelease { code },
    };
    let handle = state
        .session
        .lock()
        .await
        .clone()
        .ok_or("No active session")?;
    // Enqueuing can wait under network backpressure. Keep disconnect and local
    // permission changes available; this cloned handle cannot target a new session.
    drop(status);
    drop(_operation);
    handle
        .send_command(command)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn select_remote_monitor(
    state: State<'_, AppState>,
    session_id: String,
    index: usize,
) -> Result<(), String> {
    let _operation = state.operation.lock().await;
    let status = state.status.read().await;
    if status.state != "connected"
        || status.host
        || status.session_id.as_deref() != Some(&session_id)
    {
        return Err("No matching client session is active".into());
    }
    if !state
        .monitors
        .read()
        .await
        .iter()
        .any(|monitor| monitor.index == index)
    {
        return Err("Remote monitor is not available".into());
    }
    let handle = state
        .session
        .lock()
        .await
        .clone()
        .ok_or("No active session")?;
    drop(status);
    drop(_operation);
    handle
        .send_command(SessionCommand::MonitorSelect { index })
        .await
        .map_err(|error| error.to_string())
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
    let _operation = state.operation.lock().await;
    ensure_idle(&state).await?;
    let address: SocketAddr = address
        .parse()
        .map_err(|_| "Use a direct host:port address".to_string())?;
    if password.trim().is_empty() {
        return Err("A non-empty session password is required".into());
    }
    clear_viewer_state(&state).await;
    *state.permissions.write().await = permissions.clone();
    *state.host.write().await = false;
    set_status(
        &state,
        Status {
            state: "connecting".into(),
            detail: "Authenticating connection".into(),
            session_id: None,
            host: false,
        },
    )
    .await;
    let app_clone = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let result = SessionManager::start_client(SessionConfig {
            connect_addr: Some(address),
            ..config(password, permissions)
        })
        .await;
        finish_connection(app_clone, result, false).await;
    });
    *state.host_task.lock().await = Some(task);
    Ok(())
}

async fn ensure_idle(state: &AppState) -> Result<(), String> {
    if state.status.read().await.state != "idle"
        || state.host_task.lock().await.is_some()
        || state.session.lock().await.is_some()
    {
        return Err("Disconnect the current session before starting another".into());
    }
    Ok(())
}

async fn finish_connection(
    app: AppHandle,
    result: Result<SessionHandle, easycontrol::CoreError>,
    host: bool,
) {
    let state = app.state::<AppState>();
    let _operation = state.operation.lock().await;
    state.host_task.lock().await.take();
    match result {
        Ok(handle) => {
            let id = handle.id().to_string();
            *state.session.lock().await = Some(handle.clone());
            set_status(
                &state,
                Status {
                    state: "connected".into(),
                    detail: "Authenticated session active".into(),
                    session_id: Some(id),
                    host,
                },
            )
            .await;
            forward_events(app.clone(), handle);
        }
        Err(error) => {
            *state.host.write().await = false;
            set_status(
                &state,
                Status {
                    state: "idle".into(),
                    detail: error.to_string(),
                    session_id: None,
                    host: false,
                },
            )
            .await;
            let _ = app.emit(
                "session-event",
                FrontendEvent {
                    kind: "error".into(),
                    detail: Some(error.to_string()),
                    session_id: None,
                },
            );
        }
    }
}

#[tauri::command]
async fn start_host(
    app: AppHandle,
    state: State<'_, AppState>,
    port: u16,
    password: String,
    permissions: Permissions,
) -> Result<(), String> {
    let _operation = state.operation.lock().await;
    ensure_idle(&state).await?;
    if port < 1024 {
        return Err("Use a port from 1024 to 65535".into());
    }
    if password.trim().is_empty() {
        return Err("A non-empty session password is required".into());
    }
    // Each connection starts view-only. Input approval belongs to the host
    // after authentication, even if a previous session permitted control.
    let permissions = Permissions {
        input: false,
        ..permissions
    };
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port))
        .await
        .map_err(|error| format!("Could not listen on port {port}: {error}"))?;
    clear_viewer_state(&state).await;
    *state.permissions.write().await = permissions.clone();
    *state.host.write().await = true;
    set_status(
        &state,
        Status {
            state: "hosting".into(),
            detail: format!("Waiting for a direct connection on port {port}"),
            session_id: None,
            host: true,
        },
    )
    .await;
    let app_clone = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let result =
            SessionManager::start_host_listener(listener, config(password, permissions)).await;
        finish_connection(app_clone, result, true).await;
    });
    *state.host_task.lock().await = Some(task);
    Ok(())
}

/// Returns the active interface address that would be used for a direct LAN
/// connection. This only discovers a local address; it does not change the
/// session transport or establish a remote connection.
#[tauri::command]
fn local_network_endpoint(port: u16) -> Result<String, String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|error| error.to_string())?;
    socket
        .connect("192.0.2.1:80")
        .map_err(|error| error.to_string())?;
    let address = socket.local_addr().map_err(|error| error.to_string())?;
    if address.ip().is_unspecified() {
        return Err("No LAN address is available".into());
    }
    Ok(format!("{}:{port}", address.ip()))
}

#[tauri::command]
async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    let _operation = state.operation.lock().await;
    if let Some(task) = state.host_task.lock().await.take() {
        task.abort();
        let _ = task.await;
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
            host: false,
        },
    )
    .await;
    clear_viewer_state(&state).await;
    *state.host.write().await = false;
    Ok(())
}

#[tauri::command]
async fn available_monitors(state: State<'_, AppState>) -> Result<Vec<MonitorInfo>, String> {
    Ok(state
        .monitors
        .read()
        .await
        .iter()
        .map(|monitor| MonitorInfo {
            index: monitor.index,
            x: monitor.x,
            y: monitor.y,
            width: monitor.width,
            height: monitor.height,
            primary: monitor.primary,
        })
        .collect())
}

#[derive(Serialize)]
struct MonitorInfo {
    index: usize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    primary: bool,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    tauri::Builder::default()
        .manage(AppState::default())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let app = window.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    let _ = disconnect(app.state::<AppState>()).await;
                    app.exit(0);
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            start_host,
            local_network_endpoint,
            connect,
            disconnect,
            connection_status,
            configure_permissions,
            desktop_permissions,
            request_screen_recording_permission,
            open_privacy_settings,
            open_creator_profile,
            host_session_status,
            available_monitors,
            latest_frame,
            frame_stats,
            viewer_input,
            select_remote_monitor
        ])
        .run(tauri::generate_context!())
        .expect("failed to run EasyControl desktop");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn latest_frame_is_bounded_and_replaces_stale_pixels() {
        let state = AppState::default();
        store_frame(&state, 1, 1, 4, "RGBA8".into(), vec![1, 2, 3, 4]).await;
        store_frame(&state, 1, 1, 4, "RGBA8".into(), vec![5, 6, 7, 8]).await;
        let frames = state.frames.read().await;
        assert_eq!(frames.dropped, 1);
        let frame = frames.latest.as_ref().unwrap();
        assert_eq!(frame.sequence, 2);
        assert_eq!(encode_frame(frame, frames.dropped)[..4], *b"ECF1");
        assert_eq!(frame.data, vec![5, 6, 7, 8]);
    }

    #[tokio::test]
    async fn invalid_frame_is_not_exposed() {
        let state = AppState::default();
        store_frame(&state, 2, 1, 4, "RGBA8".into(), vec![0; 4]).await;
        assert!(state.frames.read().await.latest.is_none());
    }

    #[tokio::test]
    async fn frame_delivery_is_isolated_to_the_active_client_session() {
        let state = AppState::default();
        *state.status.write().await = Status {
            state: "connected".into(),
            detail: String::new(),
            session_id: Some("active".into()),
            host: false,
        };
        store_frame(&state, 1, 1, 4, "RGBA8".into(), vec![7; 4]).await;
        assert!(frame_packet(&state, "previous", 0).await.is_empty());
        assert_eq!(&frame_packet(&state, "active", 0).await[..4], b"ECF1");
        assert!(frame_packet(&state, "active", 1).await.is_empty());
        state.status.write().await.state = "idle".into();
        assert!(frame_packet(&state, "active", 0).await.is_empty());
    }

    #[tokio::test]
    async fn delivered_frames_are_not_counted_as_dropped_and_cleanup_clears_pixels() {
        let state = AppState::default();
        store_frame(&state, 1, 1, 4, "RGBA8".into(), vec![1; 4]).await;
        state.frames.write().await.delivered_sequence = 1;
        store_frame(&state, 1, 1, 4, "RGBA8".into(), vec![2; 4]).await;
        assert_eq!(state.frames.read().await.dropped, 0);
        clear_viewer_state(&state).await;
        assert!(state.frames.read().await.latest.is_none());
        assert!(state.monitors.read().await.is_empty());
    }

    #[tokio::test]
    async fn another_operation_is_rejected_while_connection_is_pending() {
        let state = AppState::default();
        assert!(ensure_idle(&state).await.is_ok());
        state.status.write().await.state = "connecting".into();
        assert!(ensure_idle(&state).await.is_err());
    }
}
mod permissions;
