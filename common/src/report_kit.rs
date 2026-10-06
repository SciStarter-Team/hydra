//! Building blocks shared by the monthly staff reports: months and ranges,
//! formatting, chart drawing, and the page shell with its range controls and
//! PNG download.
//!
//! **Shared files.** This module and everything under `report_kit/` are
//! byte-identical in orrery (`src/staff/report_kit`) and hydra
//! (`common/src/report_kit`), next to the reports that use them, so both
//! sites' reports look and behave the same. Change both copies together; a
//! `diff` of the two directories should always come back empty.

pub mod format;
pub mod month;
pub mod shell;
pub mod svg;
