use sailfish::TemplateOnce;

/// The page's one notification region.
///
/// Every layout renders this, so that a handler answering an htmx request can
/// put a message on the screen without knowing anything about the page it came
/// from -- a handler marks the response and static/js/notifications.js puts it
/// either here or in a nearer container that asked for its own. See
/// [`crate::middleware::error_pages::error_response`].
#[derive(Debug, Default, TemplateOnce)]
#[template(path = "site/notifications.stpl.html")]
pub struct Notifications;

/// The id [`Notifications`] renders. static/js/notifications.js places messages
/// here when no nearer container has asked for them.
pub const NOTIFICATIONS_REGION_ID: &str = "notifications";

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
}
