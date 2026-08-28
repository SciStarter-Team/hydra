use sailfish::TemplateOnce;

/// The page's one notification region.
///
/// Every layout renders this, so that a handler answering an htmx request can
/// put a message on the screen without knowing anything about the page it came
/// from. A handler does that by aiming an `<hx-partial>` at
/// [`NOTIFICATIONS_TARGET`]; because a response made only of partials
/// suppresses the main swap, the message appears without disturbing whatever
/// the request was actually targeting. See
/// [`crate::middleware::error_pages::error_response`].
#[derive(Debug, Default, TemplateOnce)]
#[template(path = "site/notifications.stpl.html")]
pub struct Notifications;

/// The selector [`Notifications`] renders, for partials that aim at it.
pub const NOTIFICATIONS_TARGET: &str = "#notifications";

/// How loudly a [`Notification`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotificationLevel {
    #[default]
    Error,
    Success,
}

impl NotificationLevel {
    /// The suffix of the `ss-notification--*` modifier class.
    pub fn class_suffix(self) -> &'static str {
        match self {
            NotificationLevel::Error => "error",
            NotificationLevel::Success => "success",
        }
    }
}

/// One message in the [`Notifications`] region.
#[derive(Debug, TemplateOnce)]
#[template(path = "site/notification.stpl.html")]
pub struct Notification<'a> {
    pub level: NotificationLevel,
    /// Optional; the message alone is enough for a short one.
    pub title: &'a str,
    pub message: &'a str,
}

impl<'a> Notification<'a> {
    pub fn error(title: &'a str, message: &'a str) -> Self {
        Notification {
            level: NotificationLevel::Error,
            title,
            message,
        }
    }

    /// Wraps this message in the partial that carries it to the region.
    ///
    /// A response body of nothing but this leaves the request's own target
    /// alone, which is what a message about a failed action wants: the page
    /// the reader was working on stays as it was.
    pub fn into_partial(self) -> Result<String, sailfish::RenderError> {
        Ok(format!(
            r#"<hx-partial hx-target="{NOTIFICATIONS_TARGET}" hx-swap="beforeend">{}</hx-partial>"#,
            self.render_once()?
        ))
    }
}
