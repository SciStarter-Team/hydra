pub mod dashboard;
pub mod email;
pub mod forum;
pub mod health;
pub mod location;
pub mod log;
pub mod sync;
pub mod user;
pub mod webauthn;

/// serde `default` helper for boolean fields that should default to `true`.
pub(crate) fn default_true() -> bool {
    true
}
