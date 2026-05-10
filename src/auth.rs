// auth.rs — HMAC-SHA256 helpers for challenge-response handshake and per-packet integrity

use hmac::{Hmac, Mac};
use sha2::Sha256;
use rand::RngCore;

type HmacSha256 = Hmac<Sha256>;

pub const HMAC_LEN:  usize = 32;
pub const NONCE_LEN: usize = 16;

pub fn generate_nonce() -> [u8; NONCE_LEN] {
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    nonce
}

// Derive a per-session key from the shared server secret and the challenge nonce.
// The client computes the same value and sends it as the AuthResponse payload,
// so the server can verify AND store it as the session key in one step.
pub fn derive_session_key(shared_secret: &[u8], nonce: &[u8]) -> [u8; HMAC_LEN] {
    let mut mac = HmacSha256::new_from_slice(shared_secret)
        .expect("HMAC accepts any key length");
    mac.update(nonce);
    mac.finalize().into_bytes().into()
}

// Append a 32-byte HMAC-SHA256 tag to packet_bytes and return the result.
pub fn sign(session_key: &[u8], packet_bytes: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(session_key)
        .expect("HMAC accepts any key length");
    mac.update(packet_bytes);
    let tag = mac.finalize().into_bytes();
    [packet_bytes, tag.as_slice()].concat()
}

// Verify the trailing 32-byte HMAC tag and return the packet bytes without it.
// Returns None on a short buffer or a tag mismatch (constant-time comparison).
pub fn verify_and_strip(session_key: &[u8], raw: &[u8]) -> Option<Vec<u8>> {
    if raw.len() < HMAC_LEN {
        return None;
    }
    let (packet_bytes, tag) = raw.split_at(raw.len() - HMAC_LEN);
    let mut mac = HmacSha256::new_from_slice(session_key)
        .expect("HMAC accepts any key length");
    mac.update(packet_bytes);
    mac.verify_slice(tag).ok()?;
    Some(packet_bytes.to_vec())
}
