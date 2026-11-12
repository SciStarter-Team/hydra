use assert2::check;

use super::super::tests::form;
use super::*;

fn pairs(sent: &[(&str, &str)]) -> Vec<(String, String)> {
    sent.iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

#[test]
fn cards_are_read_in_the_order_they_come() {
    let sent = pairs(&[
        ("name", "Squirrels"),
        ("field.b.kind", "text"),
        ("field.b.label", "Where?"),
        ("field.a.kind", "number"),
        ("field.a.label", "How many?"),
        ("field.a.integer", "on"),
        ("field.b.multiline", "on"),
    ]);

    let draft = read_draft(&sent, Ulid::new(), None);

    check!(draft.errors.is_empty());
    let labels: Vec<&str> = draft
        .form
        .fields
        .iter()
        .map(|field| field.label.as_str())
        .collect();
    check!(labels == ["Where?", "How many?"]);
    check!(
        draft.form.fields[0].kind
            == FieldKind::Text {
                multiline: true,
                placeholder: String::new()
            }
    );
    check!(
        draft.form.fields[1].kind
            == FieldKind::Number {
                min: None,
                max: None,
                integer: true
            }
    );
}

#[test]
fn a_field_without_a_key_is_keyed_by_its_question() {
    let sent = pairs(&[
        ("name", "Squirrels"),
        ("field.x.kind", "text"),
        ("field.x.label", "Which tree?"),
        ("field.y.kind", "heading"),
        ("field.z.kind", "location"),
        ("field.z.key", "where"),
    ]);

    let draft = read_draft(&sent, Ulid::new(), None);
    let keys: Vec<&str> = draft
        .form
        .fields
        .iter()
        .map(|field| field.key.as_str())
        .collect();

    check!(keys == ["which-tree", "y", "where"]);
}

#[test]
fn choices_are_written_as_they_are_read() {
    let options = read_options("gray | Gray squirrel\n\n  Red squirrel  \nblack|Black squirrel");

    check!(
        options
            == [
                ChoiceOption {
                    value: "gray".to_string(),
                    label: "Gray squirrel".to_string()
                },
                ChoiceOption {
                    value: "Red squirrel".to_string(),
                    label: "Red squirrel".to_string()
                },
                ChoiceOption {
                    value: "black".to_string(),
                    label: "Black squirrel".to_string()
                },
            ]
    );
    check!(read_options(&options_text(&options)) == options);
}

#[test]
fn what_cannot_be_kept_is_refused() {
    let sent = pairs(&[
        ("slug", "squirrels/count"),
        ("next_url", "javascript:alert(1)"),
        ("field.a.kind", "number"),
        ("field.a.label", "How many?"),
        ("field.a.min", "lots"),
        ("field.b.kind", "choice"),
        ("field.b.label", "Which kind?"),
        ("field.c.kind", "time"),
        ("field.c.label", "When?"),
        ("field.c.min", "noonish"),
        ("field.d.kind", "spreadsheet"),
    ]);

    let draft = read_draft(&sent, Ulid::new(), None);

    check!(draft.errors.len() == 7, "{:#?}", draft.errors);
    check!(
        draft
            .errors
            .iter()
            .any(|error| error.contains("needs a name"))
    );
    check!(
        draft
            .errors
            .iter()
            .any(|error| error.contains("squirrels/count"))
    );
    check!(
        draft
            .errors
            .iter()
            .any(|error| error.contains("javascript:"))
    );
    check!(
        draft
            .errors
            .iter()
            .any(|error| error.contains("“How many?”") && error.contains("lots"))
    );
    check!(
        draft
            .errors
            .iter()
            .any(|error| error.contains("offers nothing to choose"))
    );
    check!(draft.errors.iter().any(|error| error.contains("noonish")));
    check!(
        draft
            .errors
            .iter()
            .any(|error| error.contains("spreadsheet"))
    );
}

#[test]
fn what_the_editor_cannot_change_is_kept() {
    let mut kept = form();
    kept.orrery_id = Some(5);

    let sent = pairs(&[("name", "Renamed"), ("sign_in_required", "on")]);
    let draft = read_draft(&sent, kept.form_id, Some(&kept));

    check!(draft.form.orrery_id == Some(5));
    check!(draft.form.created == kept.created);
    check!(draft.form.name == "Renamed");
    check!(draft.form.sign_in_required);
    check!(!draft.form.closed);
}

#[test]
fn a_condition_and_a_map_are_read() {
    let sent = pairs(&[
        ("name", "Squirrels"),
        ("map", "on"),
        ("map_popup_fields", "kind, count\nseen"),
        ("field.a.kind", "boolean"),
        ("field.a.key", "seen"),
        ("field.a.style", "checkbox"),
        ("field.b.kind", "choice"),
        ("field.b.key", "kind"),
        ("field.b.options", "gray"),
        ("field.b.widget", "select"),
        ("field.b.show_if_field", "seen"),
        ("field.b.show_if_equals", "true"),
    ]);

    let draft = read_draft(&sent, Ulid::new(), None);

    check!(draft.errors.is_empty());
    check!(
        draft.form.fields[0].kind
            == FieldKind::Boolean {
                style: BooleanStyle::Checkbox
            }
    );
    check!(
        draft.form.fields[1].visible_if
            == Some(Condition {
                field: "seen".into(),
                equals: "true".to_string()
            })
    );
    check!(matches!(
        &draft.form.fields[1].kind,
        FieldKind::Choice {
            widget: ChoiceWidget::Select,
            ..
        }
    ));
    let popup: Vec<&str> = draft
        .form
        .map
        .as_ref()
        .unwrap()
        .popup_fields
        .iter()
        .map(FieldKey::as_str)
        .collect();
    check!(popup == ["kind", "count", "seen"]);
}

#[test]
fn every_kind_is_named_as_it_is_read() {
    for kind in Kind::ALL {
        check!(kind.as_str().parse::<Kind>() == Ok(kind));
        check!(Kind::of(&kind.blank()) == kind);
    }
}

#[test]
fn a_card_names_its_controls_by_its_token() {
    let card = FieldCard {
        token: "f3".to_string(),
        field: form().fields[2].clone(),
    };

    let html = card.render_once().unwrap();

    check!(html.contains(r#"name="field.f3.kind" value="choice""#));
    check!(html.contains(r#"name="field.f3.options""#));
    check!(html.contains("gray | Gray squirrel"));
    check!(html.contains(r#"name="field.f3.show_if_field" value="seen""#));
}
