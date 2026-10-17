//! Session crypto for BLE provisioning (docs/PROVISIONING.md, "Security").
//!
//! X25519 gives a fresh shared secret per session; HKDF mixes in the setup
//! code shown on the panel, so only someone who can read the panel derives the
//! same keys. Both sides prove that with an HMAC before any Wi-Fi data flows,
//! and everything afterwards is AES-256-GCM with per-direction keys and
//! counter nonces.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::Rng;
use sha2::{Digest, Sha256};

pub const KEY_INFO: &[u8] = b"inkframe/prov/v1";
pub const LABEL_PHONE: &[u8] = b"phone";
pub const LABEL_DEVICE: &[u8] = b"device";

/// Crockford base32: no I, L, O, U, so codes survive being read off a screen.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
pub const CODE_LEN: usize = 12;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("peer key is not a valid curve point")]
    WeakKey,
    #[error("message failed authentication")]
    Decrypt,
    #[error("message counter exhausted")]
    Exhausted,
}

/// A fresh 60-bit setup code, e.g. `7K3M9QXR2D4F`.
pub fn new_code() -> String {
    let mut rng = rand::thread_rng();
    (0..CODE_LEN)
        .map(|_| ALPHABET[rng.gen_range(0..32)] as char)
        .collect()
}

/// `7K3M9QXR2D4F` → `7K3M-9QXR-2D4F`.
pub fn display_code(code: &str) -> String {
    code.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join("-")
}

/// Canonical form of what a person typed: case-insensitive, separators
/// ignored, and the look-alikes O→0, I/L→1. `None` if it cannot be a code.
/// The device only ever compares codes it generated itself; this is the
/// reference for what the app sends (SetupCode.kt).
#[cfg(test)]
pub fn normalize_code(input: &str) -> Option<String> {
    let mut out = String::with_capacity(CODE_LEN);
    for c in input.chars() {
        let c = match c.to_ascii_uppercase() {
            ' ' | '-' => continue,
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        if !c.is_ascii() || !ALPHABET.contains(&(c as u8)) {
            return None;
        }
        out.push(c);
    }
    (out.len() == CODE_LEN).then_some(out)
}

pub struct Keys {
    pub confirm: [u8; 32],
    pub phone_to_device: [u8; 32],
    pub device_to_phone: [u8; 32],
}

/// HKDF-SHA256(salt, ikm = shared ‖ SHA-256(code), info = KEY_INFO ‖ phone_pub ‖ device_pub) → 96 bytes.
pub fn derive(
    shared: &[u8; 32],
    code: &str,
    salt: &[u8],
    phone_pub: &[u8; 32],
    device_pub: &[u8; 32],
) -> Keys {
    let mut ikm = [0u8; 64];
    ikm[..32].copy_from_slice(shared);
    ikm[32..].copy_from_slice(&Sha256::digest(code.as_bytes()));
    let hk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let info = [KEY_INFO, phone_pub, device_pub].concat();
    let mut okm = [0u8; 96];
    // 96 bytes is far below HKDF-SHA256's 8160-byte limit.
    let _ = hk.expand(&info, &mut okm);
    let mut keys = Keys {
        confirm: [0; 32],
        phone_to_device: [0; 32],
        device_to_phone: [0; 32],
    };
    keys.confirm.copy_from_slice(&okm[..32]);
    keys.phone_to_device.copy_from_slice(&okm[32..64]);
    keys.device_to_phone.copy_from_slice(&okm[64..]);
    keys
}

fn hmac(key: &[u8; 32]) -> Hmac<Sha256> {
    // HMAC zero-pads short keys to the 64-byte block, so this is exactly
    // HMAC-SHA256 with the 32-byte key, without a fallible constructor.
    let mut block = [0u8; 64];
    block[..32].copy_from_slice(key);
    <Hmac<Sha256> as KeyInit>::new(&block.into())
}

pub fn proof(confirm: &[u8; 32], label: &[u8]) -> [u8; 32] {
    let mut m = hmac(confirm);
    m.update(label);
    m.finalize().into_bytes().into()
}

/// Constant-time check of a peer's proof.
pub fn verify_proof(confirm: &[u8; 32], label: &[u8], got: &[u8]) -> bool {
    let mut m = hmac(confirm);
    m.update(label);
    m.verify_slice(got).is_ok()
}

/// One direction of the sealed channel. Nonce = 4 zero bytes ‖ u64 BE counter;
/// the transport is ordered and reliable, so the receiver simply expects the
/// next counter value and any replay, drop or reorder fails authentication.
pub struct Channel {
    cipher: Aes256Gcm,
    counter: u64,
}

impl Channel {
    pub fn new(key: &[u8; 32]) -> Self {
        Self {
            cipher: Aes256Gcm::new(key.into()),
            counter: 0,
        }
    }

    fn next_nonce(&mut self) -> Result<[u8; 12], CryptoError> {
        let n = self.counter;
        self.counter = self.counter.checked_add(1).ok_or(CryptoError::Exhausted)?;
        let mut nonce = [0u8; 12];
        nonce[4..].copy_from_slice(&n.to_be_bytes());
        Ok(nonce)
    }

    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let nonce = self.next_nonce()?;
        self.cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext)
            .map_err(|_| CryptoError::Decrypt)
    }

    pub fn open(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let nonce = self.next_nonce()?;
        self.cipher
            .decrypt(Nonce::from_slice(&nonce), ciphertext)
            .map_err(|_| CryptoError::Decrypt)
    }
}

/// X25519 agreement that refuses low-order peer points (all-zero output).
pub fn agree(
    secret: &x25519_dalek::StaticSecret,
    peer: &[u8; 32],
) -> Result<[u8; 32], CryptoError> {
    let shared = secret.diffie_hellman(&x25519_dalek::PublicKey::from(*peer));
    if !shared.was_contributory() {
        return Err(CryptoError::WeakKey);
    }
    Ok(*shared.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_normalise() {
        let code = new_code();
        assert_eq!(code.len(), CODE_LEN);
        assert_eq!(
            normalize_code(&display_code(&code)).as_deref(),
            Some(code.as_str())
        );
        assert_eq!(
            normalize_code("7k3m-9qxr-2d4f").as_deref(),
            Some("7K3M9QXR2D4F")
        );
        assert_eq!(
            normalize_code("o1il 0000 0000").as_deref(),
            Some("011100000000")
        );
        assert_eq!(normalize_code("7K3M-9QXR-2D4"), None);
        assert_eq!(normalize_code("7K3M-9QXR-2D4U"), None);
    }

    #[test]
    fn channel_rejects_replay_and_tamper() -> Result<(), CryptoError> {
        let key = [7u8; 32];
        let (mut tx, mut rx) = (Channel::new(&key), Channel::new(&key));
        let a = tx.seal(b"one")?;
        let b = tx.seal(b"two")?;
        assert_eq!(rx.open(&a)?, b"one");
        // Replaying `a` where `b` is expected fails, and so does tampering.
        assert_eq!(Channel::new(&key).open(&b), Err(CryptoError::Decrypt));
        let mut bad = b.clone();
        bad[0] ^= 1;
        assert_eq!(rx.open(&bad), Err(CryptoError::Decrypt));
        Ok(())
    }

    #[test]
    fn low_order_point_rejected() {
        let s = x25519_dalek::StaticSecret::from([9u8; 32]);
        assert_eq!(agree(&s, &[0u8; 32]), Err(CryptoError::WeakKey));
    }
}
