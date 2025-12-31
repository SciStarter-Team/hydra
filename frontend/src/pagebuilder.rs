//! Page Builder module for visual page composition
//!
//! This module provides types and functions for:
//! - Managing reusable page components (templates with slots)
//! - Building pages by placing components into slots
//! - Rendering pages in both editing and presenting modes

pub mod cache;
mod component;
mod page;
pub mod source;

pub use component::PageBuilderComponent;
pub use page::PageBuilderPage;
