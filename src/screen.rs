use crate::protocol::MonitorInfo;
use thiserror::Error;
#[cfg(feature = "capture")]
use xcap::Monitor as XcapMonitor;

#[derive(Debug, Error)]
pub enum ScreenError {
    #[error("capture failed: {0}")]
    Capture(String),
    #[error("not supported on this platform")]
    NotSupported,
    #[error("monitor {0} not found")]
    MonitorNotFound(usize),
}

pub trait ScreenCapture: Send + Sync {
    fn monitors(&self) -> Result<Vec<MonitorInfo>, ScreenError>;
    fn capture(&mut self, monitor: usize) -> Result<Vec<u8>, ScreenError>;
    fn dimensions(&self, monitor: usize) -> Result<(u32, u32), ScreenError>;
}

pub struct NullCapture;

impl NullCapture {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NullCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenCapture for NullCapture {
    fn monitors(&self) -> Result<Vec<MonitorInfo>, ScreenError> {
        Ok(vec![MonitorInfo {
            index: 0,
            x: 0,
            y: 0,
            width: 1280,
            height: 720,
            primary: true,
        }])
    }

    fn capture(&mut self, _monitor: usize) -> Result<Vec<u8>, ScreenError> {
        Err(ScreenError::NotSupported)
    }

    fn dimensions(&self, _monitor: usize) -> Result<(u32, u32), ScreenError> {
        Ok((1280, 720))
    }
}

#[cfg(feature = "capture")]
pub struct XcapCapture;

#[cfg(feature = "capture")]
impl XcapCapture {
    pub fn new() -> Result<Self, ScreenError> {
        // Probe that at least one monitor exists to validate the backend.
        let _ = XcapMonitor::all().map_err(|e| ScreenError::Capture(e.to_string()))?;
        Ok(Self)
    }
}

#[cfg(feature = "capture")]
impl Default for XcapCapture {
    fn default() -> Self {
        Self::new().unwrap_or(Self)
    }
}

#[cfg(feature = "capture")]
impl ScreenCapture for XcapCapture {
    fn monitors(&self) -> Result<Vec<MonitorInfo>, ScreenError> {
        let monitors = XcapMonitor::all().map_err(|e| ScreenError::Capture(e.to_string()))?;
        let mut out = Vec::with_capacity(monitors.len());
        for (i, mon) in monitors.into_iter().enumerate() {
            let x = mon.x().map_err(|e| ScreenError::Capture(e.to_string()))?;
            let y = mon.y().map_err(|e| ScreenError::Capture(e.to_string()))?;
            let width = mon
                .width()
                .map_err(|e| ScreenError::Capture(e.to_string()))?;
            let height = mon
                .height()
                .map_err(|e| ScreenError::Capture(e.to_string()))?;
            out.push(MonitorInfo {
                index: i,
                x,
                y,
                width,
                height,
                primary: mon
                    .is_primary()
                    .map_err(|e| ScreenError::Capture(e.to_string()))?,
            });
        }
        Ok(out)
    }

    fn capture(&mut self, monitor: usize) -> Result<Vec<u8>, ScreenError> {
        // CoreGraphics may return wallpaper while hiding other applications
        // when permission is missing. Never present that partial image as a
        // working remote desktop.
        #[cfg(target_os = "macos")]
        {
            #[link(name = "CoreGraphics", kind = "framework")]
            extern "C" {
                fn CGPreflightScreenCaptureAccess() -> bool;
            }
            if !unsafe { CGPreflightScreenCaptureAccess() } {
                return Err(ScreenError::Capture("Screen Recording permission is required. Enable EasyControl in System Settings and restart it.".into()));
            }
        }
        let monitors = XcapMonitor::all().map_err(|e| ScreenError::Capture(e.to_string()))?;
        if monitor >= monitors.len() {
            return Err(ScreenError::MonitorNotFound(monitor));
        }
        let image = monitors[monitor]
            .capture_image()
            .map_err(|e| ScreenError::Capture(e.to_string()))?;
        Ok(image.into_raw())
    }

    fn dimensions(&self, monitor: usize) -> Result<(u32, u32), ScreenError> {
        let monitors = XcapMonitor::all().map_err(|e| ScreenError::Capture(e.to_string()))?;
        if monitor >= monitors.len() {
            return Err(ScreenError::MonitorNotFound(monitor));
        }
        let mon = &monitors[monitor];
        let width = mon
            .width()
            .map_err(|e| ScreenError::Capture(e.to_string()))?;
        let height = mon
            .height()
            .map_err(|e| ScreenError::Capture(e.to_string()))?;
        // xcap reports display bounds in macOS points while capture_image()
        // returns physical Retina pixels. Frame metadata must describe the
        // byte buffer, otherwise receivers correctly reject the frame.
        #[cfg(target_os = "macos")]
        let scale = mon
            .scale_factor()
            .map_err(|e| ScreenError::Capture(e.to_string()))?;
        // Other xcap backends already report physical pixel dimensions.
        #[cfg(not(target_os = "macos"))]
        let scale = 1.0;
        Ok((
            (width as f32 * scale).round() as u32,
            (height as f32 * scale).round() as u32,
        ))
    }
}

pub struct ScreenSession {
    capture: Box<dyn ScreenCapture>,
    monitor: usize,
}

impl ScreenSession {
    pub fn new(capture: Box<dyn ScreenCapture>) -> Result<Self, ScreenError> {
        let monitors = capture.monitors()?;
        if monitors.is_empty() {
            return Err(ScreenError::Capture("no monitors".into()));
        }
        Ok(Self {
            capture,
            monitor: 0,
        })
    }

    pub fn monitor_count(&self) -> usize {
        self.capture.monitors().map(|m| m.len()).unwrap_or(0)
    }

    pub fn list_monitors(&self) -> Result<Vec<MonitorInfo>, ScreenError> {
        self.capture.monitors()
    }

    pub fn select_monitor(&mut self, index: usize) -> Result<(), ScreenError> {
        let monitors = self.capture.monitors()?;
        if index >= monitors.len() {
            return Err(ScreenError::MonitorNotFound(index));
        }
        self.monitor = index;
        Ok(())
    }

    pub fn current_monitor(&self) -> usize {
        self.monitor
    }

    pub fn capture_frame(&mut self) -> Result<Vec<u8>, ScreenError> {
        self.capture.capture(self.monitor)
    }

    pub fn current_dimensions(&self) -> Result<(u32, u32), ScreenError> {
        self.capture.dimensions(self.monitor)
    }
}

#[cfg(feature = "capture")]
pub fn default_capture() -> Box<dyn ScreenCapture> {
    Box::new(XcapCapture)
}

#[cfg(not(feature = "capture"))]
pub fn default_capture() -> Box<dyn ScreenCapture> {
    Box::new(NullCapture)
}

impl Default for ScreenSession {
    fn default() -> Self {
        Self {
            capture: default_capture(),
            monitor: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TwoMonitors;

    impl ScreenCapture for TwoMonitors {
        fn monitors(&self) -> Result<Vec<MonitorInfo>, ScreenError> {
            Ok(vec![
                MonitorInfo {
                    index: 0,
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 1,
                    primary: true,
                },
                MonitorInfo {
                    index: 1,
                    x: 2,
                    y: 0,
                    width: 3,
                    height: 1,
                    primary: false,
                },
            ])
        }
        fn capture(&mut self, monitor: usize) -> Result<Vec<u8>, ScreenError> {
            Ok(vec![
                monitor as u8;
                self.dimensions(monitor)?.0 as usize * 4
            ])
        }
        fn dimensions(&self, monitor: usize) -> Result<(u32, u32), ScreenError> {
            match monitor {
                0 => Ok((2, 1)),
                1 => Ok((3, 1)),
                _ => Err(ScreenError::MonitorNotFound(monitor)),
            }
        }
    }

    #[test]
    fn selecting_a_monitor_changes_capture_dimensions_and_rejects_invalid_indices() {
        let mut screen = ScreenSession::new(Box::new(TwoMonitors)).unwrap();
        screen.select_monitor(1).unwrap();
        assert_eq!(screen.current_monitor(), 1);
        assert_eq!(screen.current_dimensions().unwrap(), (3, 1));
        assert_eq!(screen.capture_frame().unwrap(), vec![1; 12]);
        assert!(matches!(
            screen.select_monitor(2),
            Err(ScreenError::MonitorNotFound(2))
        ));
    }

    #[cfg(all(feature = "capture", target_os = "macos"))]
    #[test]
    #[ignore = "requires a macOS Screen Recording grant"]
    fn macos_capture_dimensions_match_frame_bytes() {
        let mut capture = XcapCapture::new().expect("macOS capture backend should initialize");
        let (width, height) = capture.dimensions(0).expect("primary display dimensions");
        let bytes = capture.capture(0).expect("primary display frame");
        assert_eq!(bytes.len(), width as usize * height as usize * 4);
    }
}
