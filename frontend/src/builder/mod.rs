//! Implementation of types and operations for the "Page Builder" feature.
//!
//! This feature has several unusual requirements that impact its design and implementation:
//! - Rendering of both page templates and component templates must happen within an async context
//! - Rendering may need to produce different results depending on whether the template is being viewed in the builder interface or on the site
//!   - For example, an if statement should pick at most one branch when rendered on the site, but display all of them in the builder interface
//! - Components may need to add content to the page <head> tag, but if multiple components add the same item to the <head> it should only appear once
//! - Components may need to fetch data from the database, but if multiple components run the same query it should only run once and the result should be shared among them
//! - Components may need to pass data to nested components
//! - Components need to be placed on a page in the page builder interface by drag-and-drop from a palette, and customized by editing options via a GUI

pub mod context;
pub mod parse;
pub mod render;
pub mod value;
