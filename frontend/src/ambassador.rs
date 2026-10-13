//! The SciStarter Ambassador program's public pages: the application form
//! ([`apply`]) and the directory of certified ambassadors with their profiles
//! ([`directory`]).
//!
//! The program lives under `/ambassador`, its overview page: the application
//! is at `/ambassador/apply`, its confirmation at `/ambassador/apply/submitted`
//! and the directory at `/ambassador/directory`.
//! `/ambassadors` redirects to the overview. The addresses these pages had
//! before, orrery's `/ambassador-apply` and `/ambassadors-directory`, and the
//! plural and singular variants people have bookmarked, are `Redirect`
//! entities. Profile links carry an ID, so the old ones, orrery's
//! `/ambassador/<id>` and `/ambassadors-directory/<id>`, are
//! [`directory::legacy_profile_redirect`] instead. The first shares this
//! prefix, which is safe: the router prefers the literal `apply` and
//! `directory` segments to its `:id`, and no ID is spelled like either.

pub mod apply;
pub mod directory;

use ulid::Ulid;

/// The application form.
pub const APPLY_PATH: &str = "/ambassador/apply";

/// The confirmation a successful submission of the application form lands on.
pub const SUBMITTED_PATH: &str = "/ambassador/apply/submitted";

/// The public directory.
pub const DIRECTORY_PATH: &str = "/ambassador/directory";

/// An ambassador's public profile. Everything that links to a profile builds
/// the link here.
pub fn profile_path(user_id: Ulid) -> String {
    format!("{DIRECTORY_PATH}/{user_id}")
}
