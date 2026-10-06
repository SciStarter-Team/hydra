//! The sign-up referral report shared with orrery: which referral source a
//! new account came from, the monthly tally, and its chart.
//!
//! `sources.rs`, `report.rs`, `chart.rs` and `page.rs` are byte-identical to
//! orrery's `src/staff/referrals/`, on top of the shared report kit
//! (`report_kit/`), so the two `/staff/signup-referrals` pages draw the same
//! report from the same rules, down to the markup. Change both copies
//! together; a `diff` of the two directories should always come back empty.
//!
//! The user service tallies (`user.referrals.monthly`), since the accounts
//! are its data; the frontend renders.

pub mod chart;
pub mod page;
pub mod report;
pub mod sources;
