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

/// The controls the answers are submitted from, here and in orrery: whether
/// the event has a Facebook event, its link, and whether SciStarter should
/// create one.
pub const HAS_FIELD: &str = "facebook_event";
pub const URL_FIELD: &str = "facebook_event_url";
pub const REQUESTED_FIELD: &str = "facebook_event_requested";

/// What is wrong with the answers as a submission, and which of their
/// controls it is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Problem {
    pub field: &'static str,
    pub message: &'static str,
}

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

    /// What's wrong with the answers as a submission, if anything: both
    /// yes-or-no questions need an answer, and an organizer who says the
    /// event has a Facebook event has to link it. The same rules and words
    /// as orrery's add form (`problem` in its `ss-event-facebook`).
    pub fn problem(&self) -> Option<Problem> {
        let problem = |field, message| Some(Problem { field, message });

        match self {
            FacebookEvent::Unanswered => problem(
                HAS_FIELD,
                "Please say whether your event has a Facebook event page.",
            ),
            FacebookEvent::Missing {
                create_requested: None,
            } => problem(
                REQUESTED_FIELD,
                "Please say whether you'd like SciStarter to create a Facebook event for you.",
            ),
            FacebookEvent::Exists { url: None } => {
                problem(URL_FIELD, "Please share the link to your Facebook event.")
            }
            FacebookEvent::Exists { url: Some(url) }
                if !crate::web_address::is_web_address(url) =>
            {
                problem(
                    URL_FIELD,
                    "Please enter the full link to your Facebook event, starting with https://.",
                )
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use assert2::check;

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

    /// The field of the problem with `answers`, if there is one.
    fn problem_field(answers: FacebookEvent) -> Option<&'static str> {
        answers.problem().map(|problem| problem.field)
    }

    #[test]
    fn a_facebook_event_needs_a_web_link() {
        check!(problem_field(FacebookEvent::Exists { url: None }) == Some(URL_FIELD));
        for url in ["facebook.com/events/1", "https:www.facebook.com/events/1"] {
            check!(problem_field(FacebookEvent::from_answers("yes", url, "")) == Some(URL_FIELD));
        }
        check!(
            problem_field(FacebookEvent::from_answers(
                "yes",
                "https://www.facebook.com/events/1/",
                ""
            )) == None
        );
    }

    #[test]
    fn both_questions_need_an_answer() {
        check!(problem_field(FacebookEvent::Unanswered) == Some(HAS_FIELD));
        check!(
            problem_field(FacebookEvent::Missing {
                create_requested: None
            }) == Some(REQUESTED_FIELD)
        );
        for requested in [true, false] {
            check!(
                problem_field(FacebookEvent::Missing {
                    create_requested: Some(requested)
                }) == None
            );
        }
    }
}
