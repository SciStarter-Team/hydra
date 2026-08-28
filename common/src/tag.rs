//! Well-known user tag names.
//!
//! Most tags are data: staff invent them, campaigns carry them, and no code
//! needs to know their spelling. The few here are different — code acts on
//! them, in more than one service — so they are spelled once, where every
//! service can reach the spelling, rather than repeated as literals that can
//! drift apart without anything looking wrong.

/// The tag granted for completing the Champion training (orrery's
/// `AmbassadorState.tutorial_enter`).
pub const CHAMPION_TRAINING: &str = "scistarter-champion-training-complete";

/// The spelling [`CHAMPION_TRAINING`] had before the Champion rename.
///
/// Written by nothing, in either system, since the rename. It stays because
/// several hundred accounts finished the training under the old name and were
/// never re-tagged, so a report that asks "did this person finish the training?"
/// has to accept it — see [`CHAMPION_TRAINING_ANY`] — and the Constant Contact
/// mirror has to keep carrying it for the contacts that hold it.
pub const LEGACY_CHAMPION_TRAINING: &str = "scistarter-ambassador-training-complete";

/// Every spelling that means the Champion training is done.
///
/// Ask with this rather than with [`CHAMPION_TRAINING`] alone: on live data the
/// old spelling is the *more* common of the two, and a report that checks only
/// the current one silently misses the majority of the people it is about.
pub const CHAMPION_TRAINING_ANY: &[&str] = &[CHAMPION_TRAINING, LEGACY_CHAMPION_TRAINING];
