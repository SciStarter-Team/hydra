use sailfish::TemplateOnce;

use crate::component::floating_field::FormError;
use crate::event::facebook::FacebookEvent;

/// The questions an event's organizer is asked about promoting the event on
/// Facebook, as form controls named for [`FacebookEvent::from_answers`]:
/// [`facebook::HAS_FIELD`], [`facebook::URL_FIELD`] and
/// [`facebook::REQUESTED_FIELD`].
///
/// The add-event form and the event's Facebook editor both render this, so
/// the questions read the same wherever they are answered. Each follow-up
/// shows only under the answer it belongs to (`components/add-form.css`,
/// with no script), and `from_answers` drops the other on submission. Both
/// yes-or-no questions must be answered ([`FacebookEvent::problem`]), and
/// the one an error is about is marked invalid.
///
/// [`facebook::HAS_FIELD`]: crate::event::facebook::HAS_FIELD
/// [`facebook::URL_FIELD`]: crate::event::facebook::URL_FIELD
/// [`facebook::REQUESTED_FIELD`]: crate::event::facebook::REQUESTED_FIELD
#[derive(TemplateOnce)]
#[template(path = "site/facebook_questions.stpl.html")]
pub struct FacebookQuestions<'a> {
    pub answers: &'a FacebookEvent,
    pub error: Option<&'a FormError>,
}

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
    use assert2::check;

    use super::*;
    use crate::event::facebook::URL_FIELD;

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
        check!(html.contains(r#"value="yes" required checked"#));
        check!(html.contains(r#"value="https://www.facebook.com/events/1/""#));

        let html = render(
            &FacebookEvent::Missing {
                create_requested: Some(true),
            },
            None,
        );
        check!(html.contains(r#"name="facebook_event" value="no" required checked"#));
        check!(html.contains(r#"name="facebook_event_requested" value="yes" required checked"#));

        let html = render(&FacebookEvent::Unanswered, None);
        check!(!html.contains("checked"));
    }

    #[test]
    fn a_link_error_marks_the_link_field() {
        let error = FormError::at(URL_FIELD, "Please share the link to your Facebook event.");
        let html = render(&FacebookEvent::Exists { url: None }, Some(&error));
        check!(html.matches(r#"aria-invalid="true""#).count() == 1);
    }

    #[test]
    fn both_questions_are_required() {
        let html = render(&FacebookEvent::Unanswered, None);
        check!(html.matches("pill-required").count() == 2);
        check!(html.matches(r#"type="radio""#).count() == html.matches(" required").count());
        check!(!html.contains("aria-invalid"));
    }

    #[test]
    fn an_unanswered_question_is_marked() {
        let problem = FacebookEvent::Unanswered.problem().unwrap();
        let error = FormError::at(problem.field, problem.message);
        let html = render(&FacebookEvent::Unanswered, Some(&error));
        check!(html.contains(
            r#"name="facebook_event" value="yes" required aria-invalid="true" aria-describedby="form-error""#
        ));
        check!(html.matches(r#"aria-invalid="true""#).count() == 2);

        let missing = FacebookEvent::Missing {
            create_requested: None,
        };
        let problem = missing.problem().unwrap();
        let error = FormError::at(problem.field, problem.message);
        let html = render(&missing, Some(&error));
        check!(html.contains(
            r#"name="facebook_event_requested" value="no" required aria-invalid="true""#
        ));
        check!(html.matches(r#"aria-invalid="true""#).count() == 2);
    }
}
