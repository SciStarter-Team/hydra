//! A submission's answers, shown to people (on the map, in the table of
//! submissions) and written out (in the CSV of them).
//!
//! An answer is shown as its field asks it, a choice by its label, and
//! written as it is kept, a choice by its value, so that a spreadsheet of
//! them reads the same however the field's wording changes.

use common::message::form::{Answer, BooleanStyle, FieldKind, FormField};

/// A number as people write it: a whole number without a decimal point.
fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

/// `answer` as it is kept, in words: what the CSV of submissions holds.
pub fn written(answer: &Answer) -> String {
    match answer {
        Answer::Text(text) | Answer::Choice(text) => text.clone(),
        Answer::Number(n) => number(*n),
        Answer::Bool(value) => value.to_string(),
        Answer::Choices(values) => values.join("; "),
        Answer::Date(date) => date.to_string(),
        Answer::Time(time) => time.strftime("%H:%M").to_string(),
        Answer::DateTime(when) => when.strftime("%Y-%m-%d %H:%M").to_string(),
        Answer::Instant(instant) => instant.to_string(),
        Answer::Photos(urls) => urls.join(" "),
        Answer::Location(point) => format!("{}, {}", point.latitude, point.longitude),
        Answer::Legacy(serde_json::Value::String(text)) => text.clone(),
        Answer::Legacy(value) => value.to_string(),
    }
}

/// `answer` as `field` asks for it, for people to read: a choice by its
/// label, a yes-or-no question's answer as the question puts it. An answer
/// no field asks for any longer is shown as it is kept.
pub fn shown(field: Option<&FormField>, answer: &Answer) -> String {
    let label = |value: &str| -> String {
        match field.map(|field| &field.kind) {
            Some(FieldKind::Choice { options, .. }) => options
                .iter()
                .find(|option| option.value == value)
                .map_or_else(|| value.to_string(), |option| option.label.clone()),
            _ => value.to_string(),
        }
    };

    match answer {
        Answer::Choice(value) => label(value),
        Answer::Choices(values) => values
            .iter()
            .map(|value| label(value))
            .collect::<Vec<_>>()
            .join("; "),
        Answer::Bool(value) => {
            let true_false = matches!(
                field.map(|field| &field.kind),
                Some(FieldKind::Boolean {
                    style: BooleanStyle::TrueFalse
                })
            );

            match (true_false, value) {
                (true, true) => "True",
                (true, false) => "False",
                (false, true) => "Yes",
                (false, false) => "No",
            }
            .to_string()
        }
        Answer::Photos(urls) if urls.len() == 1 => "1 photo".to_string(),
        Answer::Photos(urls) => format!("{} photos", urls.len()),
        other => written(other),
    }
}

/// An answer as a page shows it: in words, or as the photos it is.
pub enum Display {
    Words(String),
    /// The photos' addresses, those that are links a page may show.
    Photos(Vec<String>),
}

/// `answer` as `field` asks for it, as a page shows it ([`shown`]).
pub fn display(field: Option<&FormField>, answer: &Answer) -> Display {
    match answer {
        Answer::Photos(urls) => Display::Photos(
            urls.iter()
                .filter_map(|url| super::link(url))
                .map(str::to_string)
                .collect(),
        ),
        answer => Display::Words(shown(field, answer)),
    }
}

#[cfg(test)]
mod tests {
    use assert2::check;
    use common::message::form::{ChoiceOption, ChoiceWidget};

    use super::*;

    fn choice() -> FormField {
        FormField {
            key: "kind".into(),
            label: "Which kind?".to_string(),
            help: String::new(),
            required: false,
            visible_if: None,
            remember: false,
            default: None,
            kind: FieldKind::Choice {
                options: vec![ChoiceOption {
                    value: "gray".to_string(),
                    label: "Gray squirrel".to_string(),
                }],
                multiple: true,
                widget: ChoiceWidget::Buttons,
            },
        }
    }

    #[test]
    fn a_choice_is_shown_by_its_label_and_written_by_its_value() {
        let answer = Answer::Choices(vec!["gray".to_string(), "albino".to_string()]);

        check!(shown(Some(&choice()), &answer) == "Gray squirrel; albino");
        check!(written(&answer) == "gray; albino");
    }

    #[test]
    fn a_whole_number_is_written_without_a_point() {
        check!(written(&Answer::Number(3.0)) == "3");
        check!(written(&Answer::Number(2.5)) == "2.5");
    }

    #[test]
    fn an_answer_kept_from_orrery_is_written_as_it_was() {
        check!(written(&Answer::Legacy(serde_json::json!("~170"))) == "~170");
        check!(written(&Answer::Legacy(serde_json::json!({"a": 1}))) == r#"{"a":1}"#);
    }
}
