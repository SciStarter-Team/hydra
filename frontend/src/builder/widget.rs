//! Page-builder components implemented in Rust.
//!
//! The template language is a glue layer. It has no data access, no field
//! access, and no expressions beyond a variable lookup, and that is deliberate:
//! the moment it grows them, every new data need grows the language further,
//! and the markup ends up in an untyped string in a database row where nothing
//! checks it.
//!
//! A widget is the alternative. It does its work in Rust — fetching, filtering,
//! formatting — against a Sailfish template the compiler checks, and the builder
//! composes it into a page like any other component.
//!
//! # Why a widget is not an ordinary component
//!
//! Placing an ordinary page-builder component **renders it to HTML there and
//! then** and splices that HTML into the page source
//! ([`crate::pagebuilder::source::compose_component_into_slot`]). The slot tag
//! left behind records what was placed; it is not a live reference. That is
//! fine for a card whose text was typed in by hand, and fatal for anything that
//! has to reflect data — it would freeze on the day it was placed.
//!
//! A widget escapes that with the `^` literal escape. A component whose source
//! reads
//!
//! ```text
//! {$ param groups label="Filter Groups" type="textarea" $}
//! {$^ widget webinar_list groups="*groups" $}
//! ```
//!
//! bakes down to `{$ widget webinar_list groups="…" $}` in the page source —
//! the directive survives placement, with the staff-supplied param interpolated
//! into it by the `*groups` form, and runs again on every request.
//!
//! # Configuration
//!
//! Attributes are strings, exactly like slot params, and are handed to the
//! widget as-is. Interpreting them is the widget's job.

use std::sync::Arc;

use anyhow::Error;
use futures::future::BoxFuture;

use super::context::Context;

/// The configuration a `{$ widget $}` directive carries, as written.
pub struct WidgetParams<'a>(&'a [(String, String)]);

impl<'a> WidgetParams<'a> {
    pub fn new(params: &'a [(String, String)]) -> Self {
        WidgetParams(params)
    }

    pub fn get(&self, key: &str) -> Option<&'a str> {
        self.0
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    pub fn get_or_empty(&self, key: &str) -> &'a str {
        self.get(key).unwrap_or("")
    }
}

/// A page-builder component written in Rust.
pub trait Widget: Send + Sync + 'static {
    /// The name `{$ widget name $}` refers to. Must be unique.
    fn name(&self) -> &'static str;

    /// What the editor calls this widget when offering it.
    fn label(&self) -> &'static str;

    /// Renders the widget.
    ///
    /// `ctx` carries the database pool and the viewing user; `params` carries
    /// the directive's attributes. Returns the HTML to splice into the page.
    fn render<'a>(
        &'a self,
        ctx: &'a Arc<Context>,
        params: WidgetParams<'a>,
    ) -> BoxFuture<'a, Result<String, Error>>;
}

/// One widget's entry in the registry.
///
/// Registration is by [`inventory`], so a widget is declared where it is
/// defined and there is no central list to fall out of date.
pub struct WidgetRegistration {
    pub widget: &'static dyn Widget,
}

inventory::collect!(WidgetRegistration);

/// Looks up a widget by the name used in a `{$ widget $}` directive.
pub fn lookup(name: &str) -> Option<&'static dyn Widget> {
    inventory::iter::<WidgetRegistration>()
        .find(|registration| registration.widget.name() == name)
        .map(|registration| registration.widget)
}

/// Every registered widget, for the editor to offer.
pub fn all() -> Vec<&'static dyn Widget> {
    inventory::iter::<WidgetRegistration>()
        .map(|registration| registration.widget)
        .collect()
}

/// Registers a widget so `{$ widget name $}` can find it.
///
/// ```ignore
/// pub struct WebinarList;
/// impl Widget for WebinarList { /* ... */ }
/// register_widget!(WebinarList);
/// ```
#[macro_export]
macro_rules! register_widget {
    ($widget:expr) => {
        inventory::submit! {
            $crate::builder::widget::WidgetRegistration { widget: &$widget }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_widget_names_are_unique() {
        let mut names: Vec<&str> = all().iter().map(|widget| widget.name()).collect();
        let count = names.len();

        names.sort_unstable();
        names.dedup();

        assert_eq!(
            names.len(),
            count,
            "two widgets registered under the same name; `lookup` would silently pick one"
        );
    }

    #[test]
    fn every_registered_widget_is_reachable_by_name() {
        for widget in all() {
            assert!(
                lookup(widget.name()).is_some(),
                "{} registered but not found by lookup",
                widget.name()
            );
        }
    }

    #[test]
    fn an_unregistered_name_finds_nothing() {
        assert!(lookup("no_such_widget_exists").is_none());
    }
}
