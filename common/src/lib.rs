// The `handler!` macro names paths through `common::`, so that services
// can use it. Aliasing the crate to itself lets this crate use it too.
extern crate self as common;

pub mod badge;
pub mod channel;
pub mod contribution_time;
pub mod elect;
pub mod email;
pub mod foundations;
pub mod insensitivestring;
pub mod linked_platform;
pub mod message;
#[cfg(feature = "database")]
pub mod misc;
pub mod organizations;
#[cfg(feature = "panoptes")]
pub mod panoptes;
pub mod primary;
#[cfg(feature = "database")]
pub mod query;
pub mod quill;
pub mod referrals;
pub mod report_kit;
pub mod sequence;
pub mod serde_utils;
pub mod service;
pub mod shared;
pub mod tag;
#[cfg(test)]
mod test_nats;
pub mod timestring;
#[cfg(feature = "database")]
pub mod transaction;
pub mod ulid;
pub mod webauthn;
