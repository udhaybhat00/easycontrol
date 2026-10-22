use crate::protocol::MonitorInfo;
#[cfg(feature = "capture")]
use xcap::Monitor as XcapMonitor;
use thiserror::Error;

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
            let x = mon.x().map_err(|e| ScreenError::Capture(e.to_string()))? as i32;
            let y = mon.y().map_err(|e| ScreenError::Capture(e.to_string()))? as i32;
            let width = mon.width().map_err(|e| ScreenError::Capture(e.to_string()))?;
            let height = mon.height().map_err(|e| ScreenError::Capture(e.to_string()))?;
            out.push(MonitorInfo {
                index: i,
                x,
                y,
                width,
                height,
                primary: i == 0,
            });
        }
        Ok(out)
    }

    fn capture(&mut self, monitor: usize) -> Result<Vec<u8>, ScreenError> {
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
        let width = mon.width().map_err(|e| ScreenError::Capture(e.to_string()))?;
        let height = mon.height().map_err(|e| ScreenError::Capture(e.to_string()))?;
        Ok((width, height))
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
        Ok(Self { capture, monitor: 0 })
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
fn default_capture() -> Box<dyn ScreenCapture> {
    Box::new(XcapCapture::default())
}

#[cfg(not(feature = "capture"))]
fn default_capture() -> Box<dyn ScreenCapture> {
    Box::new(NullCapture)
}

impl Default for ScreenSession {
    fn default() -> Self {
        Self { capture: default_capture(), monitor: 0 }
    }
}
