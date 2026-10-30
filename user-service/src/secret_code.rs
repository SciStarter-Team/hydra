//! Unguessable strings: the codes in emailed links, pending signups, and API
//! keys.

use base64::Engine;
use rand::Rng;

/// The size of an emailed or pending-signup code.
pub const CODE_BYTES: usize = 32;

/// The size of an API key, matching orrery's `secrets.token_urlsafe(64)`.
pub const API_KEY_BYTES: usize = 64;

/// `bytes` random bytes, URL-safe base64 encoded without padding.
pub fn generate(bytes: usize) -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..bytes).map(|_| rng.r#gen()).collect();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes)
}
