use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPermissions {
    pub platform: String,
    pub screen_recording: String,
    pub accessibility: String,
    pub input_monitoring: String,
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

pub fn status() -> DesktopPermissions {
    #[cfg(target_os = "macos")]
    unsafe {
        DesktopPermissions {
            platform: "macOS".into(),
            screen_recording: if CGPreflightScreenCaptureAccess() {
                "granted"
            } else {
                "required"
            }
            .into(),
            accessibility: if AXIsProcessTrusted() {
                "granted"
            } else {
                "required"
            }
            .into(),
            // Input injection on macOS is governed by Accessibility. Input
            // Monitoring is only required for global event observation, which
            // EasyControl does not perform.
            input_monitoring: "not_required".into(),
        }
    }
    #[cfg(not(target_os = "macos"))]
    DesktopPermissions {
        platform: std::env::consts::OS.into(),
        screen_recording: "platform_specific".into(),
        accessibility: "platform_specific".into(),
        input_monitoring: "platform_specific".into(),
    }
}

pub fn request_screen_recording() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        CGRequestScreenCaptureAccess()
    }
    #[cfg(not(target_os = "macos"))]
    false
}

pub fn open_settings(kind: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let url = match kind {
            "screen" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
            "accessibility" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            _ => return Err("unknown privacy settings pane".into()),
        };
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|error| error.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    Err("open settings is currently supported on macOS only".into())
}
