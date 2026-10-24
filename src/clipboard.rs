//! Clipboard adapters. The protocol carries only bounded UTF-8 text.
use thiserror::Error;

pub const MAX_CLIPBOARD_BYTES: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("clipboard is unavailable: {0}")]
    Unavailable(String),
    #[error("clipboard payload exceeds {MAX_CLIPBOARD_BYTES} bytes")]
    TooLarge,
    #[error("clipboard text is not valid UTF-8")]
    InvalidText,
}

pub trait ClipboardBackend: Send {
    fn get_text(&mut self) -> Result<String, ClipboardError>;
    fn set_text(&mut self, text: &str) -> Result<(), ClipboardError>;
}

/// Opens the OS clipboard for each operation. This avoids keeping platform UI
/// resources alive across sessions and reports permission/backend errors to the
/// caller instead of panicking.
pub struct SystemClipboard;

impl ClipboardBackend for SystemClipboard {
    fn get_text(&mut self) -> Result<String, ClipboardError> {
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|error| ClipboardError::Unavailable(error.to_string()))?;
        let text = clipboard
            .get_text()
            .map_err(|error| ClipboardError::Unavailable(error.to_string()))?;
        validate_text(&text)?;
        Ok(text)
    }

    fn set_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        validate_text(text)?;
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|error| ClipboardError::Unavailable(error.to_string()))?;
        clipboard
            .set_text(text)
            .map_err(|error| ClipboardError::Unavailable(error.to_string()))
    }
}

pub fn validate_bytes(data: &[u8]) -> Result<&str, ClipboardError> {
    if data.len() > MAX_CLIPBOARD_BYTES {
        return Err(ClipboardError::TooLarge);
    }
    std::str::from_utf8(data).map_err(|_| ClipboardError::InvalidText)
}

pub fn validate_text(text: &str) -> Result<(), ClipboardError> {
    if text.len() > MAX_CLIPBOARD_BYTES {
        return Err(ClipboardError::TooLarge);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_or_oversized_protocol_payloads() {
        assert!(matches!(
            validate_bytes(&[0xff]),
            Err(ClipboardError::InvalidText)
        ));
        assert!(matches!(
            validate_bytes(&vec![b'x'; MAX_CLIPBOARD_BYTES + 1]),
            Err(ClipboardError::TooLarge)
        ));
        assert!(validate_bytes(b"bounded text").is_ok());
    }
}
