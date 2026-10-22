use chacha20poly1305::{ChaCha20Poly1305, KeyInit as _};
use chacha20poly1305::aead::{Aead, Nonce};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("cipher error")]
    Cipher,
    #[error("mac mismatch")]
    MacMismatch,
    #[error("invalid length")]
    InvalidLength,
}

const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const MAC_LEN: usize = 32;
const KEY_LEN: usize = 32;

pub struct SessionCipher {
    enc_key: [u8; KEY_LEN],
    dec_key: [u8; KEY_LEN],
    enc_mac_key: [u8; KEY_LEN],
    dec_mac_key: [u8; KEY_LEN],
    enc_counter: u64,
    dec_counter: u64,
}

impl SessionCipher {
    pub fn new(material: &[u8; 128]) -> Self {
        let (rest, enc_mac_key) = material.split_at(KEY_LEN);
        let (rest, dec_mac_key) = rest.split_at(KEY_LEN);
        let (rest, enc_key) = rest.split_at(KEY_LEN);
        let (_rest, dec_key) = rest.split_at(KEY_LEN);

        Self {
            enc_key: enc_key.try_into().unwrap(),
            dec_key: dec_key.try_into().unwrap(),
            enc_mac_key: enc_mac_key.try_into().unwrap(),
            dec_mac_key: dec_mac_key.try_into().unwrap(),
            enc_counter: 0,
            dec_counter: 0,
        }
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);

        let cipher = ChaCha20Poly1305::new_from_slice(&self.enc_key).unwrap();
        let nonce_obj = Nonce::<ChaCha20Poly1305>::from_slice(&nonce);
        let mut ct = cipher.encrypt(nonce_obj, plaintext).expect("chacha encrypt");
        ct.extend_from_slice(&nonce);

        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.enc_mac_key).unwrap();
        mac.update(&ct);
        let tag = mac.finalize().into_bytes();

        let mut out = Vec::with_capacity(ct.len() + tag.len());
        out.extend_from_slice(&ct);
        out.extend_from_slice(&tag);

        self.enc_counter = self.enc_counter.wrapping_add(1);
        out
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if ciphertext.len() < NONCE_LEN + TAG_LEN + MAC_LEN {
            return Err(CryptoError::InvalidLength);
        }

        let payload_len = ciphertext.len() - MAC_LEN;
        let (payload, tag) = ciphertext.split_at(payload_len);

        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.dec_mac_key).unwrap();
        mac.update(payload);
        let expected = mac.finalize().into_bytes();
        if tag != expected.as_slice() {
            return Err(CryptoError::MacMismatch);
        }

        let nonce_start = payload.len() - NONCE_LEN;
        let (ct, nonce) = payload.split_at(nonce_start);

        let cipher = ChaCha20Poly1305::new_from_slice(&self.dec_key).unwrap();
        let nonce_obj = Nonce::<ChaCha20Poly1305>::from_slice(nonce);
        let pt = cipher
            .decrypt(nonce_obj, ct)
            .map_err(|_| CryptoError::Cipher)?;

        self.dec_counter = self.dec_counter.wrapping_add(1);
        Ok(pt)
    }
}

pub fn derive_session_key(secret: &[u8], salt: &[u8]) -> [u8; 128] {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(secret);
    let mut out = [0u8; 128];
    let mut block = [0u8; KEY_LEN];
    let mut counter: u64 = 0;
    let mut pos = 0;
    while pos < out.len() {
        let mut h = Sha256::new();
        h.update(&block);
        h.update(&counter.to_le_bytes());
        let digest = h.finalize();
        let take = (out.len() - pos).min(digest.len());
        out[pos..pos + take].copy_from_slice(&digest[..take]);
        pos += take;
        block.copy_from_slice(&digest);
        counter = counter.wrapping_add(1);
    }
    out
}

pub fn fingerprint(key: &[u8; KEY_LEN]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key);
    let digest = hasher.finalize();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&digest[..8])
}
