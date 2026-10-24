use base64::Engine as _;
use chacha20poly1305::aead::{Aead, Nonce as AeadNonce};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit as _};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const MAC_LEN: usize = 32;
const KEY_LEN: usize = 32;
const KEY_MATERIAL_LEN: usize = 128;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("cipher error")]
    Cipher,
    #[error("mac mismatch")]
    MacMismatch,
    #[error("invalid length")]
    InvalidLength,
}

pub struct SessionCipher {
    enc_key: [u8; KEY_LEN],
    dec_key: [u8; KEY_LEN],
    enc_mac_key: [u8; KEY_LEN],
    dec_mac_key: [u8; KEY_LEN],
}

impl SessionCipher {
    pub fn new(material: &[u8; KEY_MATERIAL_LEN]) -> Self {
        let mut enc_key = [0u8; KEY_LEN];
        let mut dec_key = [0u8; KEY_LEN];
        let mut enc_mac_key = [0u8; KEY_LEN];
        let mut dec_mac_key = [0u8; KEY_LEN];
        enc_mac_key.copy_from_slice(&material[0..KEY_LEN]);
        dec_mac_key.copy_from_slice(&material[KEY_LEN..2 * KEY_LEN]);
        enc_key.copy_from_slice(&material[2 * KEY_LEN..3 * KEY_LEN]);
        dec_key.copy_from_slice(&material[3 * KEY_LEN..4 * KEY_LEN]);

        Self {
            enc_key,
            dec_key,
            enc_mac_key,
            dec_mac_key,
        }
    }

    /// Client uses the opposite directional keys from the host.
    pub fn for_role(material: &[u8; KEY_MATERIAL_LEN], is_host: bool) -> Self {
        let mut cipher = Self::new(material);
        if !is_host {
            std::mem::swap(&mut cipher.enc_key, &mut cipher.dec_key);
            std::mem::swap(&mut cipher.enc_mac_key, &mut cipher.dec_mac_key);
        }
        cipher
    }

    /// Encrypt plaintext. Returns ct(plaintext + chacha_tag) + nonce(12) + mac_tag(32).
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; NONCE_LEN];
        getrandom(&mut nonce).expect("failed to get random bytes");

        let cipher = ChaCha20Poly1305::new_from_slice(&self.enc_key).unwrap();
        let nonce_obj = AeadNonce::<ChaCha20Poly1305>::from_slice(&nonce);
        let mut ct = cipher
            .encrypt(nonce_obj, plaintext)
            .expect("chacha encrypt");
        ct.extend_from_slice(&nonce);

        let mut mac = <HmacSha256 as hmac::Mac>::new_from_slice(&self.enc_mac_key).unwrap();
        mac.update(&ct);
        let tag = mac.finalize().into_bytes();

        let mut out = Vec::with_capacity(ct.len() + tag.len());
        out.extend_from_slice(&ct);
        out.extend_from_slice(&tag);

        out
    }

    /// Decrypt ciphertext produced by encrypt. Returns plaintext.
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if ciphertext.len() < NONCE_LEN + TAG_LEN + MAC_LEN {
            return Err(CryptoError::InvalidLength);
        }

        let payload_len = ciphertext.len() - MAC_LEN;
        let (payload, tag) = ciphertext.split_at(payload_len);

        let mut mac = <HmacSha256 as hmac::Mac>::new_from_slice(&self.dec_mac_key).unwrap();
        mac.update(payload);
        mac.verify_slice(tag)
            .map_err(|_| CryptoError::MacMismatch)?;

        let nonce_start = payload.len() - NONCE_LEN;
        let (ct, nonce) = payload.split_at(nonce_start);

        let cipher = ChaCha20Poly1305::new_from_slice(&self.dec_key).unwrap();
        let nonce_obj = AeadNonce::<ChaCha20Poly1305>::from_slice(nonce);
        let pt = cipher
            .decrypt(nonce_obj, ct)
            .map_err(|_| CryptoError::Cipher)?;

        Ok(pt)
    }
}

/// Derive session key material using HKDF-SHA256 (RFC 5869).
pub fn derive_session_key(ikm: &[u8], salt: &[u8], info: &[u8]) -> [u8; KEY_MATERIAL_LEN] {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut okm = [0u8; KEY_MATERIAL_LEN];
    hk.expand(info, &mut okm).expect("HKDF expand failed");
    okm
}

/// Compute a fingerprint of a public key for display/verification.
pub fn fingerprint(key: &[u8; KEY_LEN]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key);
    let digest = hasher.finalize();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&digest[..8])
}

use getrandom::getrandom;

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a cipher where all four keys are equal (for testing only).
    /// In the real protocol, host encrypts with enc_key/decrypts with dec_key,
    /// while client does the opposite.
    fn test_cipher() -> SessionCipher {
        let mut material = [0u8; KEY_MATERIAL_LEN];
        getrandom(&mut material[..KEY_LEN]).unwrap();
        let seed = material[..KEY_LEN].to_vec();
        material[KEY_LEN..2 * KEY_LEN].copy_from_slice(&seed);
        material[2 * KEY_LEN..3 * KEY_LEN].copy_from_slice(&seed);
        material[3 * KEY_LEN..4 * KEY_LEN].copy_from_slice(&seed);
        SessionCipher::new(&material)
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let mut cipher = test_cipher();
        let plaintext = b"hello world";
        let ct = cipher.encrypt(plaintext);
        assert_ne!(ct, plaintext);
        let pt = cipher.decrypt(&ct).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn mac_tampering_rejected() {
        let mut cipher = test_cipher();

        let pt = b"tamper me";
        let mut ct = cipher.encrypt(pt);

        // Flip a byte in the ciphertext portion
        ct[0] ^= 0x01;
        assert!(matches!(cipher.decrypt(&ct), Err(CryptoError::MacMismatch)));
    }

    #[test]
    fn derive_session_key_deterministic() {
        let ikm = b"test ikm material";
        let salt = b"test salt";
        let info = b"test info";
        let k1 = derive_session_key(ikm, salt, info);
        let k2 = derive_session_key(ikm, salt, info);
        assert_eq!(k1, k2);
        assert_ne!(k1, [0u8; KEY_MATERIAL_LEN]);
    }

    #[test]
    fn fingerprint_deterministic() {
        let key = [1u8; 32];
        let f1 = fingerprint(&key);
        let f2 = fingerprint(&key);
        assert_eq!(f1, f2);
        assert_eq!(f1.len(), 11); // base64 of 8 bytes = 11 chars
    }

    #[test]
    fn invalid_length_rejected() {
        let mut cipher = test_cipher();
        assert!(matches!(
            cipher.decrypt(&[0u8; 5]),
            Err(CryptoError::InvalidLength)
        ));
    }
}
