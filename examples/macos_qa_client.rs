//! Development-only loopback client for controlled real macOS input QA.
//! No window/event listener, so injected keys cannot feed back into this client.
//! Never bundled with the desktop app. Copy the host's password immediately
//! before launch; it is read once into memory and never printed or persisted.

#[cfg(target_os = "macos")]
mod qa {
    use easycontrol::clipboard::{ClipboardBackend, SystemClipboard};
    use easycontrol::{SessionCommand as Command, SessionConfig, SessionEvent, SessionManager};
    use std::time::Instant;
    use tokio::io::{AsyncBufReadExt, BufReader};

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceFlagsState(state_id: i32) -> u64;
    }

    pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
        if std::env::var("EASYCONTROL_OS_QA").as_deref() != Ok("1") {
            return Err(
                "Set EASYCONTROL_OS_QA=1 only for an explicitly approved blank-document test"
                    .into(),
            );
        }
        let password = SystemClipboard.get_text()?;
        if password.len() != 14 || !password.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err("Copy the generated host password immediately before starting QA".into());
        }
        let session = SessionManager::start_client(SessionConfig {
            connect_addr: Some("127.0.0.1:5899".parse()?),
            password: Some(password),
            enable_capture: false,
            ..SessionConfig::default()
        })
        .await?;
        let mut events = session.event_stream();
        println!("AUTHENTICATED. View-only until the host approves control. No input is sent automatically.");
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        let started = Instant::now();
        let mut frames = 0u64;
        let mut bytes = 0u64;
        let mut monitors = Vec::new();
        let mut commands = 0u32;
        loop {
            tokio::select! {
                event = events.recv() => match event {
                    Ok(SessionEvent::ScreenFrame { data, .. }) => { frames += 1; bytes += data.len() as u64; },
                    Ok(SessionEvent::MonitorList { monitors: inventory }) => { monitors = inventory; println!("MONITORS {}",monitors.len()); },
                    Ok(SessionEvent::PermissionDenied { channel, .. }) => println!("DENIED {channel:?}"),
                    Ok(SessionEvent::Disconnected { .. }) => { println!("DISCONNECTED"); break; },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => println!("EVENT_LAG"),
                    Err(_) => break,
                    _ => {},
                },
                line = lines.next_line() => {
                    let Some(line) = line? else { break; };
                    let parts: Vec<_> = line.split_whitespace().collect();
                    if parts.first() == Some(&"quit") { break; }
                    if parts.first() == Some(&"stats") {
                        println!("RECEIVED frames={frames} rgba_bytes={bytes} elapsed_ms={} shift_down={}",started.elapsed().as_millis(),unsafe {CGEventSourceFlagsState(1)} & (1<<17) != 0);
                        continue;
                    }
                    commands += 1;
                    if commands > 24 { println!("Input command budget reached"); break; }
                    let inputs = match parts.as_slice() {
                        ["move", x, y] => {
                            let (x,y): (i32,i32) = (x.parse()?,y.parse()?);
                            if !monitors.iter().any(|m| x as i64 >= m.x as i64 && y as i64 >= m.y as i64 && (x as i64) < m.x as i64 + m.width as i64 && (y as i64) < m.y as i64 + m.height as i64) {
                                println!("Rejected point outside remote monitor bounds");continue;
                            }
                            vec![Command::MouseMove{x,y}]
                        },
                        ["click"] => vec![Command::MouseClick{button:1}],
                        ["right-click"] => vec![Command::MouseClick{button:2}],
                        ["scroll"] => vec![Command::MouseScroll{dx:0,dy:3}],
                        ["type"] => vec![Command::KeyType{text:"EasyControl QA 123".into()}],
                        ["shift-a"] => vec![Command::KeyPress{code:0x11},Command::KeyPress{code:65},Command::KeyRelease{code:65},Command::KeyRelease{code:0x11}],
                        ["backspace"] => vec![Command::KeyPress{code:8},Command::KeyRelease{code:8}],
                        ["escape"] => vec![Command::KeyPress{code:0x1b},Command::KeyRelease{code:0x1b}],
                        ["shift-down"] => vec![Command::KeyPress{code:0x11}],
                        _ => {println!("Unknown QA command");continue;},
                    };
                    for input in inputs { session.send_command(input).await?; }
                    println!("SENT {} (host policy still applies)",parts[0]);
                }
            }
        }
        session.disconnect().await?;
        println!("CLEAN SHUTDOWN");
        Ok(())
    }
}

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    qa::run().await
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This development-only OS-input QA client is macOS-specific.");
}
