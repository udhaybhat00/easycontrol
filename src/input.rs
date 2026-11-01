use crate::protocol::InputMsg;
#[cfg(feature = "input")]
use enigo::{Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use std::collections::{BTreeMap, BTreeSet};
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
            InputMsg::MouseClick { button } => {
                let button = match button {
                    1 => Button::Left,
                    2 => Button::Right,
                    3 => Button::Middle,
                    _ => return Err(InputError::Injection("invalid mouse button".into())),
                };
                enigo
                    .button(button, Direction::Click)
                    .map_err(|e| InputError::Injection(e.to_string()))?;
            }
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
                    _ => return Err(InputError::Injection("invalid mouse button".into())),
                };
                enigo
                    .button(btn, Direction::Press)
                    .map_err(|e| InputError::Injection(e.to_string()))?;
            }
            InputMsg::MouseRelease { button, x, y } => {
                enigo
                    .move_mouse(*x, *y, Coordinate::Abs)
                    .map_err(|e| InputError::Injection(e.to_string()))?;
                let btn = match *button {
                    1 => Button::Left,
                    2 => Button::Right,
                    3 => Button::Middle,
                    _ => return Err(InputError::Injection("invalid mouse button".into())),
                };
                enigo
                    .button(btn, Direction::Release)
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
                let key = key_from_code(*code)
                    .ok_or_else(|| InputError::Injection("unsupported key".into()))?;
                enigo
                    .key(key, Direction::Press)
                    .map_err(|e| InputError::Injection(e.to_string()))?;
            }
            InputMsg::KeyRelease { code } => {
                let key = key_from_code(*code)
                    .ok_or_else(|| InputError::Injection("unsupported key".into()))?;
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
fn key_from_code(code: u16) -> Option<Key> {
    // Browser KeyboardEvent.code represents physical keys. On macOS, passing
    // Unicode through Enigo performs a TIS keyboard-layout lookup; recent macOS
    // versions assert when that lookup runs on a Tokio worker. Native ANSI
    // keycodes preserve physical-key semantics without touching the layout API.
    #[cfg(target_os = "macos")]
    if let Some(native) = macos_physical_key(code) {
        return Some(Key::Other(native));
    }
    Some(match code {
        0x08 => Key::Backspace,
        0x09 => Key::Tab,
        0x0D => Key::Return,
        0x10 => Key::Control,
        0x11 => Key::Shift,
        0x12 => Key::Alt,
        0x14 => Key::CapsLock,
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
        #[cfg(not(target_os = "macos"))]
        0x2D => Key::Insert,
        0x2E => Key::Delete,
        0x70 => Key::F1,
        0x71 => Key::F2,
        0x72 => Key::F3,
        0x73 => Key::F4,
        0x74 => Key::F5,
        0x75 => Key::F6,
        0x76 => Key::F7,
        0x77 => Key::F8,
        0x78 => Key::F9,
        0x79 => Key::F10,
        0x7A => Key::F11,
        0x7B => Key::F12,
        0xBA => Key::Unicode(';'),
        0xBB => Key::Unicode('='),
        0xBC => Key::Unicode(','),
        0xBD => Key::Unicode('-'),
        0xBE => Key::Unicode('.'),
        0xBF => Key::Unicode('/'),
        0xC0 => Key::Unicode('`'),
        0xDB => Key::Unicode('['),
        0xDC => Key::Unicode('\\'),
        0xDD => Key::Unicode(']'),
        0xDE => Key::Unicode('\''),
        0x30..=0x39 => Key::Unicode(char::from_u32(code as u32).unwrap_or('0')),
        0x41..=0x5A => Key::Unicode((code as u8 as char).to_ascii_lowercase()),
        _ => return None,
    })
}

#[cfg(all(feature = "input", target_os = "macos"))]
fn macos_physical_key(code: u16) -> Option<u32> {
    // Apple's HIToolbox Events.h kVK_ANSI_* constants, indexed by v5 wire codes.
    const LETTERS: [u32; 26] = [
        0x00, 0x0b, 0x08, 0x02, 0x0e, 0x03, 0x05, 0x04, 0x22, 0x26, 0x28, 0x25, 0x2e, 0x2d, 0x1f,
        0x23, 0x0c, 0x0f, 0x01, 0x11, 0x20, 0x09, 0x0d, 0x07, 0x10, 0x06,
    ];
    const DIGITS: [u32; 10] = [0x1d, 0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1a, 0x1c, 0x19];
    Some(match code {
        0x41..=0x5a => LETTERS[(code - 0x41) as usize],
        0x30..=0x39 => DIGITS[(code - 0x30) as usize],
        0xba => 0x29,
        0xbb => 0x18,
        0xbc => 0x2b,
        0xbd => 0x1b,
        0xbe => 0x2f,
        0xbf => 0x2c,
        0xc0 => 0x32,
        0xdb => 0x21,
        0xdc => 0x2a,
        0xdd => 0x1e,
        0xde => 0x27,
        _ => return None,
    })
}

#[derive(Default)]
struct HeldInput {
    keys: BTreeSet<u16>,
    buttons: BTreeMap<u8, (i32, i32)>,
}

pub struct InputSession {
    injector: Box<dyn InputInjector>,
    enabled: bool,
    held: Mutex<HeldInput>,
}

impl InputSession {
    /// Fallible OS backend initialization, so permission failures do not panic.
    pub fn try_default() -> Result<Self, InputError> {
        #[cfg(feature = "input")]
        let injector: Box<dyn InputInjector> = Box::new(EnigoInjector::new()?);
        #[cfg(not(feature = "input"))]
        let injector: Box<dyn InputInjector> = Box::new(NullInjector);
        Ok(Self::new(injector))
    }

    pub fn new(injector: Box<dyn InputInjector>) -> Self {
        Self {
            injector,
            enabled: false,
            held: Mutex::new(HeldInput::default()),
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if !enabled {
            self.release_all();
        }
        self.enabled = enabled;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn inject(&self, msg: &InputMsg) -> Result<(), InputError> {
        if !self.enabled {
            return Err(InputError::Injection("input disabled".into()));
        }
        let mut held = self
            .held
            .lock()
            .map_err(|error| InputError::Injection(error.to_string()))?;
        self.injector.inject(msg)?;
        match msg {
            InputMsg::KeyPress { code } => {
                held.keys.insert(*code);
            }
            InputMsg::KeyRelease { code } => {
                held.keys.remove(code);
            }
            InputMsg::MousePress { button, x, y } => {
                held.buttons.insert(*button, (*x, *y));
            }
            InputMsg::MouseRelease { button, .. } => {
                held.buttons.remove(button);
            }
            InputMsg::MouseMove { x, y } => {
                for point in held.buttons.values_mut() {
                    *point = (*x, *y);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn release_all(&self) {
        if let Ok(mut held) = self.held.lock() {
            for code in std::mem::take(&mut held.keys) {
                let _ = self.injector.inject(&InputMsg::KeyRelease { code });
            }
            for (button, (x, y)) in std::mem::take(&mut held.buttons) {
                let _ = self
                    .injector
                    .inject(&InputMsg::MouseRelease { button, x, y });
            }
        }
    }
}

impl Drop for InputSession {
    fn drop(&mut self) {
        self.release_all();
    }
}

impl Default for InputSession {
    fn default() -> Self {
        // A disabled placeholder must never initialize an OS injector or panic
        // on a missing grant. Call try_default() for an actual approved backend.
        Self::new(Box::new(NullInjector))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    struct Recorder(Arc<Mutex<Vec<InputMsg>>>);
    impl InputInjector for Recorder {
        fn inject(&self, msg: &InputMsg) -> Result<(), InputError> {
            self.0.lock().unwrap().push(msg.clone());
            Ok(())
        }
    }
    #[test]
    fn revocation_releases_held_keys_and_buttons_and_rejects_more_input() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut session = InputSession::new(Box::new(Recorder(events.clone())));
        session.set_enabled(true);
        session.inject(&InputMsg::KeyPress { code: 65 }).unwrap();
        session
            .inject(&InputMsg::MousePress {
                button: 1,
                x: 10,
                y: 20,
            })
            .unwrap();
        session.set_enabled(false);
        assert!(session.inject(&InputMsg::KeyPress { code: 66 }).is_err());
        let events = events.lock().unwrap();
        assert!(matches!(events[2], InputMsg::KeyRelease { code: 65 }));
        assert!(matches!(
            events[3],
            InputMsg::MouseRelease {
                button: 1,
                x: 10,
                y: 20
            }
        ));
        assert_eq!(events.len(), 4);
    }
    #[test]
    fn disabled_default_has_no_os_backend_or_permission_requirement() {
        let session = InputSession::default();
        assert!(!session.is_enabled());
        assert!(session.inject(&InputMsg::KeyPress { code: 65 }).is_err());
    }

    #[test]
    fn dropping_input_releases_a_held_key() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut session = InputSession::new(Box::new(Recorder(events.clone())));
        session.set_enabled(true);
        session.inject(&InputMsg::KeyPress { code: 0x11 }).unwrap();
        drop(session);
        assert!(matches!(
            events.lock().unwrap()[1],
            InputMsg::KeyRelease { code: 0x11 }
        ));
    }
    #[cfg(all(feature = "input", not(target_os = "macos")))]
    #[test]
    fn letters_are_lowercase_and_unsupported_keys_are_rejected() {
        assert_eq!(key_from_code(65), Some(Key::Unicode('a')));
        assert_eq!(key_from_code(0xBA), Some(Key::Unicode(';')));
        assert_eq!(key_from_code(0xDE), Some(Key::Unicode('\'')));
        assert_eq!(key_from_code(0xffff), None);
    }
    #[cfg(all(feature = "input", target_os = "macos"))]
    #[test]
    fn macos_printable_keys_never_use_worker_unsafe_layout_lookup() {
        for code in (0x41..=0x5a).chain(0x30..=0x39).chain([
            0xba, 0xbb, 0xbc, 0xbd, 0xbe, 0xbf, 0xc0, 0xdb, 0xdc, 0xdd, 0xde,
        ]) {
            assert!(
                matches!(key_from_code(code), Some(Key::Other(_))),
                "wire code {code:x}"
            );
        }
        assert_eq!(key_from_code(0x41), Some(Key::Other(0x00)));
        assert_eq!(key_from_code(0x53), Some(Key::Other(0x01)));
        assert_eq!(key_from_code(0x5a), Some(Key::Other(0x06)));
        assert_eq!(key_from_code(0x30), Some(Key::Other(0x1d)));
        assert_eq!(key_from_code(0xba), Some(Key::Other(0x29)));
        assert_eq!(key_from_code(0x11), Some(Key::Shift));
        assert_eq!(key_from_code(0xffff), None);
    }
}
