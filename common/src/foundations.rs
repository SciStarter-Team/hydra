//! The Foundations completions report shared with orrery: how many people
//! finish the Foundations of Citizen Science training each month, EvolveMe's
//! share of them, and the running total the training page shows.
//!
//! `report.rs`, `chart.rs` and `page.rs` are byte-identical to orrery's
//! `src/staff/foundations/`, on top of the shared report kit
//! (`report_kit/`). Change both copies together.
//!
//! The user service counts (`user.foundations.monthly`), since the progress
//! records are its data; the frontend renders.

pub mod chart;
pub mod page;
pub mod report;
