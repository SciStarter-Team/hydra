//! What an event's organizer said about promoting it on Facebook.
//!
//! The add-event form asks whether the event has a Facebook event of its own.
//! If it does, SciStarter offers to share it and asks for the link; if it
//! doesn't, SciStarter offers to create one. Only the follow-up that belongs
//! to the answer given is kept, which [`FacebookEvent`] makes structural: the
//! link exists only on `Exists`, the request only on `Missing`.

/// The value a yes/no answer is submitted as, here and in orrery, whose
/// `facebook_event` and `facebook_event_requested` attributes are choices
/// keyed by these strings. orrery's sync sends them through as stored.
const YES: &str = "yes";
const NO: &str = "no";

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FacebookEvent {
    /// The organizer hasn't said whether the event has a Facebook event.
    #[default]
    Unanswered,

    /// The event has a Facebook event of its own, for SciStarter to share.
    /// The link is required by the form, but an event synchronized from
    /// orrery may have been answered there without one.
    Exists { url: Option<String> },

    /// The event has no Facebook event. `create_requested` is whether the
    /// organizer asked SciStarter to create one, `None` until they say.
    Missing { create_requested: Option<bool> },
}

/// Parse a yes/no answer, treating anything else as unanswered.
fn yes_no(answer: &str) -> Option<bool> {
    match answer.trim() {
        YES => Some(true),
        NO => Some(false),
        _ => None,
    }
}

impl FacebookEvent {
    /// Assemble the answers as they are submitted: whether the event has a
    /// Facebook event, its link, and whether SciStarter should create one,
    /// each answer `"yes"`, `"no"` or empty. The follow-up that doesn't
    /// belong to the first answer is ignored, so one left behind by a change
    /// of mind is dropped rather than stored.
    pub fn from_answers(has: &str, url: &str, requested: &str) -> Self {
        match yes_no(has) {
            None => FacebookEvent::Unanswered,
            Some(true) => FacebookEvent::Exists {
                url: Some(url.trim())
                    .filter(|url| !url.is_empty())
                    .map(String::from),
            },
            Some(false) => FacebookEvent::Missing {
                create_requested: yes_no(requested),
            },
        }
    }

    /// Rebuild the value from the `events` table's three columns.
    pub fn from_columns(has: Option<bool>, url: Option<String>, requested: Option<bool>) -> Self {
        match has {
            None => FacebookEvent::Unanswered,
            Some(true) => FacebookEvent::Exists { url },
            Some(false) => FacebookEvent::Missing {
                create_requested: requested,
            },
        }
    }

    /// Whether the organizer says the event has a Facebook event: the
    /// `events` table's `facebook_event` column.
    pub fn has(&self) -> Option<bool> {
        match self {
            FacebookEvent::Unanswered => None,
            FacebookEvent::Exists { .. } => Some(true),
            FacebookEvent::Missing { .. } => Some(false),
        }
    }

    /// The event's Facebook event link: the `facebook_event_url` column.
    pub fn url(&self) -> Option<&str> {
        match self {
            FacebookEvent::Exists { url } => url.as_deref(),
            _ => None,
        }
    }

    /// Whether the organizer asked SciStarter to create a Facebook event:
    /// the `facebook_event_requested` column.
    pub fn create_requested(&self) -> Option<bool> {
        match self {
            FacebookEvent::Missing { create_requested } => *create_requested,
            _ => None,
        }
    }

    /// What's wrong with the answers as a submission, if anything: an
    /// organizer who says the event has a Facebook event has to link it.
    pub fn problem(&self) -> Option<&'static str> {
        match self {
            FacebookEvent::Exists { url: None } => {
                Some("Please share the link to your Facebook event.")
            }
            FacebookEvent::Exists { url: Some(url) } if !is_web_link(url) => {
                Some("Please enter the full link to your Facebook event, starting with https://.")
            }
            _ => None,
        }
    }
}

/// Whether `url` is an http(s) link with no spaces in it, which is what
/// orrery's form asks of the same answer.
fn is_web_link(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && !url.contains(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_follow_up_to_the_answer_is_kept() {
        assert_eq!(
            FacebookEvent::from_answers("yes", " https://fb.me/e/1 ", "yes"),
            FacebookEvent::Exists {
                url: Some("https://fb.me/e/1".to_string())
            }
        );
        assert_eq!(
            FacebookEvent::from_answers("no", "https://fb.me/e/1", "yes"),
            FacebookEvent::Missing {
                create_requested: Some(true)
            }
        );
        assert_eq!(
            FacebookEvent::from_answers("", "https://fb.me/e/1", "yes"),
            FacebookEvent::Unanswered
        );
    }

    #[test]
    fn blank_follow_ups_are_unanswered() {
        assert_eq!(
            FacebookEvent::from_answers("yes", "  ", ""),
            FacebookEvent::Exists { url: None }
        );
        assert_eq!(
            FacebookEvent::from_answers("no", "", ""),
            FacebookEvent::Missing {
                create_requested: None
            }
        );
        assert_eq!(
            FacebookEvent::from_answers("no", "", "no"),
            FacebookEvent::Missing {
                create_requested: Some(false)
            }
        );
    }

    #[test]
    fn columns_round_trip() {
        for value in [
            FacebookEvent::Unanswered,
            FacebookEvent::Exists { url: None },
            FacebookEvent::Exists {
                url: Some("https://www.facebook.com/events/1/".to_string()),
            },
            FacebookEvent::Missing {
                create_requested: None,
            },
            FacebookEvent::Missing {
                create_requested: Some(true),
            },
            FacebookEvent::Missing {
                create_requested: Some(false),
            },
        ] {
            let rebuilt = FacebookEvent::from_columns(
                value.has(),
                value.url().map(String::from),
                value.create_requested(),
            );
            assert_eq!(rebuilt, value);
        }
    }

    #[test]
    fn a_facebook_event_needs_a_web_link() {
        assert!(FacebookEvent::Exists { url: None }.problem().is_some());
        assert!(
            FacebookEvent::from_answers("yes", "facebook.com/events/1", "")
                .problem()
                .is_some()
        );
        assert!(
            FacebookEvent::from_answers("yes", "https://www.facebook.com/events/1/", "")
                .problem()
                .is_none()
        );
        assert!(FacebookEvent::Unanswered.problem().is_none());
        assert!(
            FacebookEvent::Missing {
                create_requested: None
            }
            .problem()
            .is_none()
        );
    }
}
