use crate::protocol::InputMsg;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InputError {
    #[error("injection failed: {0}")]
    Injection(String),
    #[error("not supported on this platform")]
    NotSupported,
}

pub trait InputInjector: Send + Sync {
    fn inject(&self, msg: &InputMsg) -> Result<(), InputError>;
}

pub struct NullInjector;

impl NullInjector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NullInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl InputInjector for NullInjector {
    fn inject(&self, _msg: &InputMsg) -> Result<(), InputError> {
        Err(InputError::NotSupported)
    }
}

pub struct InputSession {
    injector: Box<dyn InputInjector>,
    enabled: bool,
}

impl InputSession {
    pub fn new(injector: Box<dyn InputInjector>) -> Self {
        Self { injector, enabled: false }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn inject(&self, msg: &InputMsg) -> Result<(), InputError> {
        if !self.enabled {
            return Err(InputError::Injection("input disabled".into()));
        }
        self.injector.inject(msg)
    }
}

impl Default for InputSession {
    fn default() -> Self {
        Self {
            injector: Box::new(NullInjector),
            enabled: false,
        }
    }
}
