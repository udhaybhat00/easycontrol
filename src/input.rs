use crate::protocol::InputMsg;
#[cfg(feature = "input")]
use enigo::{Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use std::sync::Mutex;
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

#[cfg(feature = "input")]
pub struct EnigoInjector {
    enigo: Mutex<Enigo>,
}

#[cfg(feature = "input")]
impl EnigoInjector {
    pub fn new() -> Result<Self, InputError> {
        Enigo::new(&Settings::default())
            .map(|e| Self {
                enigo: Mutex::new(e),
            })
            .map_err(|e| InputError::Injection(e.to_string()))
    }
}

#[cfg(feature = "input")]
impl Default for EnigoInjector {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| Self {
            enigo: Mutex::new(Enigo::new(&Settings::default()).expect("enigo must initialize")),
        })
    }
}

#[cfg(feature = "input")]
impl InputInjector for EnigoInjector {
    fn inject(&self, msg: &InputMsg) -> Result<(), InputError> {
        let mut enigo = self
            .enigo
            .lock()
            .map_err(|e| InputError::Injection(e.to_string()))?;
        match msg {
            InputMsg::MouseMove { x, y } => {
                enigo
                    .move_mouse(*x, *y, Coordinate::Abs)
                    .map_err(|e| InputError::Injection(e.to_string()))?;
            }
            InputMsg::MousePress { button, x, y } => {
                enigo
                    .move_mouse(*x, *y, Coordinate::Abs)
                    .map_err(|e| InputError::Injection(e.to_string()))?;
                let btn = match *button {
                    1 => Button::Left,
                    2 => Button::Right,
                    3 => Button::Middle,
                    _ => Button::Left,
                };
                enigo
                    .button(btn, Direction::Click)
                    .map_err(|e| InputError::Injection(e.to_string()))?;
            }
            InputMsg::MouseRelease { button: _, x, y } => {
                enigo
                    .move_mouse(*x, *y, Coordinate::Abs)
                    .map_err(|e| InputError::Injection(e.to_string()))?;
            }
            InputMsg::MouseScroll { dx, dy } => {
                if *dx != 0 {
                    enigo
                        .scroll(*dx, enigo::Axis::Horizontal)
                        .map_err(|e| InputError::Injection(e.to_string()))?;
                }
                if *dy != 0 {
                    enigo
                        .scroll(*dy, enigo::Axis::Vertical)
                        .map_err(|e| InputError::Injection(e.to_string()))?;
                }
            }
            InputMsg::KeyPress { code } => {
                let key = key_from_code(*code);
                enigo
                    .key(key, Direction::Press)
                    .map_err(|e| InputError::Injection(e.to_string()))?;
            }
            InputMsg::KeyRelease { code } => {
                let key = key_from_code(*code);
                enigo
                    .key(key, Direction::Release)
                    .map_err(|e| InputError::Injection(e.to_string()))?;
            }
            InputMsg::KeyType { text } => {
                enigo
                    .text(text)
                    .map_err(|e| InputError::Injection(e.to_string()))?;
            }
        }
        Ok(())
    }
}

#[cfg(feature = "input")]
fn key_from_code(code: u16) -> Key {
    match code {
        0x08 => Key::Backspace,
        0x09 => Key::Tab,
        0x0D => Key::Return,
        0x10 => Key::Control,
        0x11 => Key::Shift,
        0x12 => Key::Alt,
        0x1B => Key::Escape,
        0x20 => Key::Space,
        0x21 => Key::PageUp,
        0x22 => Key::PageDown,
        0x23 => Key::End,
        0x24 => Key::Home,
        0x25 => Key::LeftArrow,
        0x26 => Key::UpArrow,
        0x27 => Key::RightArrow,
        0x28 => Key::DownArrow,
        0x2D => Key::Unicode(' '), // Insert not available on macOS
        0x2E => Key::Delete,
        0x30..=0x39 => Key::Unicode(char::from_u32(code as u32).unwrap_or('0')),
        0x41..=0x5A => Key::Unicode(char::from_u32(code as u32).unwrap_or('A')),
        _ => Key::Unicode(' '),
    }
}

pub struct InputSession {
    injector: Box<dyn InputInjector>,
    enabled: bool,
}

impl InputSession {
    pub fn new(injector: Box<dyn InputInjector>) -> Self {
        Self {
            injector,
            enabled: false,
        }
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

#[cfg(feature = "input")]
fn default_injector() -> Box<dyn InputInjector> {
    Box::new(EnigoInjector::default())
}

#[cfg(not(feature = "input"))]
fn default_injector() -> Box<dyn InputInjector> {
    Box::new(NullInjector)
}

impl Default for InputSession {
    fn default() -> Self {
        Self {
            injector: default_injector(),
            enabled: false,
        }
    }
}
