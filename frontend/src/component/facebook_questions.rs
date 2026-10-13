use sailfish::TemplateOnce;

use crate::component::floating_field::FormError;
use crate::event::facebook::FacebookEvent;

/// The questions an event's organizer is asked about promoting the event on
/// Facebook, as form controls named for [`FacebookEvent::from_answers`]:
/// `facebook_event`, `facebook_event_url` and `facebook_event_requested`.
///
/// The add-event form and the event's Facebook editor both render this, so
/// the questions read the same wherever they are answered. Each follow-up
/// shows only under the answer it belongs to (`components/add-form.css`,
/// with no script), and `from_answers` drops the other on submission.
#[derive(TemplateOnce)]
#[template(path = "site/facebook_questions.stpl.html")]
pub struct FacebookQuestions<'a> {
    pub answers: &'a FacebookEvent,
    pub error: Option<&'a FormError>,
}

/// The control the link question's error is reported against.
pub const URL_FIELD: &str = "facebook_event_url";

/// The answers to [`FacebookQuestions`] as a form submits them. A form that
/// carries the questions among others flattens this into its own fields.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct FacebookAnswers {
    facebook_event: String,
    facebook_event_url: String,
    facebook_event_requested: String,
}

impl FacebookAnswers {
    /// What the answers say, with any follow-up the first answer doesn't
    /// call for dropped.
    pub fn facebook_event(&self) -> FacebookEvent {
        FacebookEvent::from_answers(
            &self.facebook_event,
            &self.facebook_event_url,
            &self.facebook_event_requested,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(answers: &FacebookEvent, error: Option<&FormError>) -> String {
        FacebookQuestions { answers, error }.render_once().unwrap()
    }

    #[test]
    fn answers_are_checked_and_filled_in() {
        let html = render(
            &FacebookEvent::Exists {
                url: Some("https://www.facebook.com/events/1/".to_string()),
            },
            None,
        );
        assert!(html.contains(r#"value="yes" checked"#));
        assert!(html.contains(r#"value="https://www.facebook.com/events/1/""#));

        let html = render(
            &FacebookEvent::Missing {
                create_requested: Some(true),
            },
            None,
        );
        assert!(html.contains(r#"name="facebook_event" value="no" checked"#));
        assert!(html.contains(r#"name="facebook_event_requested" value="yes" checked"#));

        let html = render(&FacebookEvent::Unanswered, None);
        assert!(!html.contains("checked"));
    }

    #[test]
    fn a_link_error_marks_the_link_field() {
        let error = FormError::at(URL_FIELD, "Please share the link to your Facebook event.");
        let html = render(&FacebookEvent::Exists { url: None }, Some(&error));
        assert!(html.contains(r#"aria-invalid="true""#));
    }
}
