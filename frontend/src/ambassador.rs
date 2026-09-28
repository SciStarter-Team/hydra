//! The SciStarter Ambassador program's public pages: the application form
//! ([`apply`]) and the directory of certified ambassadors with their profiles
//! ([`directory`]).
//!
//! The paths are orrery's canonical spellings, which these pages have to
//! keep: it serves the application at `/ambassador-apply` and the directory at
//! `/ambassadors-directory`, while `/ambassador` is the program's overview page
//! and `/ambassadors` redirects to it. The plural and singular variants people
//! have bookmarked are `Redirect` entities; orrery's `/ambassador/<id>` profile
//! links are [`directory::legacy_profile_redirect`].

pub mod apply;
pub mod directory;

use ulid::Ulid;

/// The public directory.
pub const DIRECTORY_PATH: &str = "/ambassadors-directory";

/// An ambassador's public profile. Everything that links to a profile builds
/// the link here.
pub fn profile_path(user_id: Ulid) -> String {
    format!("{DIRECTORY_PATH}/{user_id}")
}
