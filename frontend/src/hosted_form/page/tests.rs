use assert2::check;
use common::message::form::RawAnswers;

use super::super::tests::form;
use super::*;

fn values(pairs: &[(&str, &str)]) -> RawAnswers {
    let mut values = RawAnswers::new();
    for (key, value) in pairs {
        values
            .entry(key.to_string())
            .or_default()
            .push(value.to_string());
    }
    values
}

#[test]
fn only_the_fields_controls_are_answers() {
    let sent = [
        (control_name(&"kind"), "gray".to_string()),
        (control_name(&"count"), "2".to_string()),
        (control_name(&"count"), "3".to_string()),
        ("next".to_string(), "/elsewhere".to_string()),
        ("csrf".to_string(), "token".to_string()),
    ];

    check!(sent_answers(&sent) == values(&[("kind", "gray"), ("count", "2"), ("count", "3")]));
}

#[test]
fn a_field_shown_by_an_answer_is_shown_only_when_given_it() {
    let form = form();

    // A heading, the checkbox, the kind it shows, and the count.
    check!(visibility(&form, &values(&[])) == [true, true, false, true]);
    check!(visibility(&form, &values(&[("seen", "true")])) == [true, true, true, true]);
    check!(visibility(&form, &values(&[("seen", "false")])) == [true, true, false, true]);
}

#[test]
fn an_unticked_box_answers_no() {
    let mut form = form();
    if let Some(condition) = &mut form.fields[2].visible_if {
        condition.equals = "false".to_string();
    }

    check!(visibility(&form, &values(&[])) == [true, true, true, true]);
    check!(visibility(&form, &values(&[("seen", "true")])) == [true, true, false, true]);
}

#[test]
fn a_field_shown_by_a_hidden_one_is_hidden_too() {
    let mut form = form();
    form.fields[3].visible_if = Some(common::message::form::Condition {
        field: "kind".into(),
        equals: "gray".to_string(),
    });

    // The kind is answered, but its question is hidden, so the count is too.
    check!(visibility(&form, &values(&[("kind", "gray")])) == [true, true, false, false]);
    check!(
        visibility(&form, &values(&[("seen", "true"), ("kind", "gray")]))
            == [true, true, true, true]
    );
}
