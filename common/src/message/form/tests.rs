use assert2::check;
use serde_json::json;

use super::*;

fn field(key: &str, kind: FieldKind) -> FormField {
    FormField {
        key: key.into(),
        label: key.to_string(),
        help: String::new(),
        required: false,
        visible_if: None,
        remember: false,
        default: None,
        kind,
    }
}

fn required(field: FormField) -> FormField {
    FormField {
        required: true,
        ..field
    }
}

fn shown_if(field: FormField, on: &str, equals: &str) -> FormField {
    FormField {
        visible_if: Some(Condition {
            field: on.into(),
            equals: equals.to_string(),
        }),
        ..field
    }
}

fn choice(options: &[&str], multiple: bool) -> FieldKind {
    FieldKind::Choice {
        options: options
            .iter()
            .map(|value| ChoiceOption {
                value: value.to_string(),
                label: value.to_uppercase(),
            })
            .collect(),
        multiple,
        widget: ChoiceWidget::Buttons,
    }
}

fn number(min: Option<f64>, max: Option<f64>, integer: bool) -> FieldKind {
    FieldKind::Number { min, max, integer }
}

fn time(not_before: Option<&str>) -> FieldKind {
    FieldKind::Time {
        min: None,
        max: None,
        step_minutes: None,
        not_before: not_before.map(FieldKey::from),
    }
}

fn text() -> FieldKind {
    FieldKind::Text {
        multiline: false,
        placeholder: String::new(),
    }
}

fn form(fields: Vec<FormField>) -> FormDefinition {
    FormDefinition {
        form_id: Ulid::nil(),
        orrery_id: None,
        slug: None,
        name: "Test".to_string(),
        intro: String::new(),
        aside: String::new(),
        image: String::new(),
        fields,
        closed: false,
        sign_in_required: true,
        anonymized: false,
        credits_participation: true,
        repetitions_field: None,
        next_url: String::new(),
        confirmation_message: String::new(),
        first_submission_email: None,
        project: None,
        owners: Vec::new(),
        map: None,
        created: TimeString::now(),
        updated: TimeString::now(),
    }
}

fn raw(pairs: &[(&str, &str)]) -> RawAnswers {
    let mut raw = RawAnswers::new();

    for (key, value) in pairs {
        raw.entry(key.to_string())
            .or_default()
            .push(value.to_string());
    }

    raw
}

fn today() -> civil::Date {
    civil::date(2026, 11, 11)
}

fn errors(result: Result<Answers, Vec<FieldError>>) -> Vec<(String, String)> {
    result
        .unwrap_err()
        .into_iter()
        .map(|error| (error.key.0, error.message))
        .collect()
}

#[test]
fn answers_are_read_as_their_fields_kinds() {
    let form = form(vec![
        field("heading", FieldKind::Heading),
        field(
            "name",
            FieldKind::Text {
                multiline: false,
                placeholder: String::new(),
            },
        ),
        field("count", number(Some(0.0), Some(50.0), true)),
        field("trees", choice(&["0", "1", "2"], false)),
        field("food", choice(&["seeds", "nuts"], true)),
        field(
            "seen",
            FieldKind::Boolean {
                style: BooleanStyle::YesNo,
            },
        ),
        field(
            "agree",
            FieldKind::Boolean {
                style: BooleanStyle::Checkbox,
            },
        ),
        field(
            "date",
            FieldKind::Date {
                not_in_future: true,
            },
        ),
        field("start", time(None)),
        field("photo", FieldKind::Photo { multiple: false }),
        field("where", FieldKind::Location),
    ]);

    let answers = form
        .read_submission(
            &raw(&[
                ("heading", "ignored"),
                ("name", "  Ada  "),
                ("count", "3"),
                ("trees", "1"),
                ("food", "nuts"),
                ("food", "seeds"),
                ("food", "nuts"),
                ("seen", "false"),
                ("date", "2026-11-10"),
                ("start", "07:30"),
                ("photo", "https://media.scistarter.org/a.jpg"),
                ("where", "41.87, -87.62"),
            ]),
            today(),
        )
        .unwrap();

    check!(answers.get("heading") == None);
    check!(answers["name"] == Answer::Text("Ada".to_string()));
    check!(answers["count"] == Answer::Number(3.0));
    check!(answers["trees"] == Answer::Choice("1".to_string()));
    check!(answers["food"] == Answer::Choices(vec!["nuts".to_string(), "seeds".to_string()]));
    check!(answers["seen"] == Answer::Bool(false));
    check!(
        answers["agree"] == Answer::Bool(false),
        "an unticked box is no"
    );
    check!(answers["date"] == Answer::Date(civil::date(2026, 11, 10)));
    check!(answers["start"] == Answer::Time(civil::time(7, 30, 0, 0)));
    check!(
        answers["photo"] == Answer::Photos(vec!["https://media.scistarter.org/a.jpg".to_string()])
    );
    check!(
        form.location(&answers)
            == Some(Point {
                latitude: 41.87,
                longitude: -87.62
            })
    );
}

#[test]
fn what_a_field_does_not_accept_is_refused_with_its_reason() {
    let form = form(vec![
        field("count", number(Some(0.0), Some(50.0), true)),
        field("ratio", number(None, None, false)),
        field("trees", choice(&["0", "1"], false)),
        field(
            "date",
            FieldKind::Date {
                not_in_future: true,
            },
        ),
        field("photo", FieldKind::Photo { multiple: false }),
        field("where", FieldKind::Location),
    ]);

    check!(
        errors(form.read_submission(
            &raw(&[
                ("count", "2.5"),
                ("ratio", "lots"),
                ("trees", "7"),
                ("date", "2026-11-12"),
                ("photo", "javascript:alert(1)"),
                ("where", "91,0"),
            ]),
            today(),
        )) == [
            (
                "count".to_string(),
                "This needs to be a whole number.".to_string()
            ),
            (
                "ratio".to_string(),
                "This needs to be a number.".to_string()
            ),
            (
                "trees".to_string(),
                "Please choose one of the options given.".to_string()
            ),
            (
                "date".to_string(),
                "This can't be in the future.".to_string()
            ),
            (
                "photo".to_string(),
                "Please upload the photo again.".to_string()
            ),
            (
                "where".to_string(),
                "Please pick a place on the map.".to_string()
            ),
        ]
    );

    check!(
        errors(form.read_submission(&raw(&[("count", "51")]), today()))
            == [(
                "count".to_string(),
                "This can't be more than 50.".to_string()
            )]
    );
}

#[test]
fn a_hidden_field_is_neither_asked_nor_required() {
    let form = form(vec![
        required(field("membership", choice(&["Yes", "No"], false))),
        required(shown_if(
            field(
                "organization",
                FieldKind::Text {
                    multiline: false,
                    placeholder: String::new(),
                },
            ),
            "membership",
            "Yes",
        )),
    ]);

    let answers = form
        .read_submission(
            &raw(&[("membership", "No"), ("organization", "Leftover")]),
            today(),
        )
        .unwrap();
    check!(answers.get("organization") == None, "dropped while hidden");

    check!(
        errors(form.read_submission(&raw(&[("membership", "Yes")]), today()))
            == [("organization".to_string(), "This is required.".to_string())]
    );

    check!(
        errors(form.read_submission(&raw(&[]), today()))
            == [("membership".to_string(), "This is required.".to_string())],
        "nothing hangs on an unanswered condition"
    );
}

#[test]
fn an_end_may_not_be_before_its_start() {
    let form = form(vec![
        field("start_time", time(None)),
        field("end_time", time(Some("start_time"))),
    ]);

    check!(
        errors(form.read_submission(
            &raw(&[("start_time", "10:00"), ("end_time", "09:00")]),
            today()
        )) == [(
            "end_time".to_string(),
            "This can't be earlier than start_time.".to_string()
        )]
    );
    check!(
        form.read_submission(
            &raw(&[("start_time", "10:00"), ("end_time", "10:00")]),
            today()
        )
        .is_ok()
    );
}

#[test]
fn repetitions_come_from_the_named_field() {
    let mut form = form(vec![field("rounds", number(None, None, true))]);
    let answers: Answers = [("rounds".into(), Answer::Number(3.0))]
        .into_iter()
        .collect();

    check!(form.repetitions(&answers) == 1, "no field named");
    form.repetitions_field = Some("rounds".into());
    check!(form.repetitions(&answers) == 3);
    check!(form.repetitions(&Answers::new()) == 1, "unanswered");
}

#[test]
fn orrerys_answers_are_read_as_the_fields_kinds() {
    let squirrel = form(vec![
        field("gray_squirrel", number(Some(0.0), Some(50.0), true)),
        field("nut_trees", choice(&["0", "1", "2"], false)),
        field("food", choice(&["seeds", "nuts"], true)),
        field(
            "date",
            FieldKind::Date {
                not_in_future: true,
            },
        ),
        field("squirrel_time", time(None)),
        field("Time seen", time(None)),
        field("science_photo", FieldKind::Photo { multiple: false }),
        field("Where", FieldKind::Location),
        field("amino_acids_pre", number(None, None, false)),
    ]);

    let read =
        |key: &str, value: serde_json::Value| squirrel.field(key).unwrap().legacy_answer(&value);

    check!(read("gray_squirrel", json!(2)) == Some(Answer::Number(2.0)));
    check!(
        read("amino_acids_pre", json!("2.5")) == Some(Answer::Number(2.5)),
        "counts kept as text"
    );
    check!(
        read("nut_trees", json!(1)) == Some(Answer::Choice("1".to_string())),
        "a coded choice"
    );
    check!(read("food", json!(["nuts"])) == Some(Answer::Choices(vec!["nuts".to_string()])));
    check!(
        read("food", json!("")) == None,
        "orrery's unanswered choice of several"
    );
    check!(
        read("date", json!("2026-10-25T04:00:00.000Z"))
            == Some(Answer::Date(civil::date(2026, 10, 25))),
        "west of Greenwich"
    );
    check!(
        read("date", json!("2026-10-24T22:00:00.000Z"))
            == Some(Answer::Date(civil::date(2026, 10, 25))),
        "east of Greenwich"
    );
    check!(read("squirrel_time", json!("07:30")) == Some(Answer::Time(civil::time(7, 30, 0, 0))));
    check!(
        read("Time seen", json!("2026-11-10T18:27:06.000Z"))
            == Some(Answer::Instant("2026-11-10T18:27:06Z".parse().unwrap())),
        "a time stored as an instant stays one"
    );
    check!(
        read("science_photo", json!("/media/squirrel.jpg"))
            == Some(Answer::Photos(vec![
                "https://scistarter.com/media/squirrel.jpg".to_string()
            ]))
    );
    check!(
        read("Where", json!([-111.934984, 33.4213]))
            == Some(Answer::Location(Point {
                latitude: 33.4213,
                longitude: -111.934984
            }))
    );
    check!(
        read("nut_trees", json!("maybe")) == Some(Answer::Legacy(json!("maybe"))),
        "what can't be read is kept"
    );
    check!(read("gray_squirrel", json!(null)) == None);
}

#[test]
fn answers_no_field_asks_for_are_found() {
    let form = form(vec![
        field("Heading", FieldKind::Heading),
        field("count", number(None, None, true)),
    ]);
    let answers: Answers = [
        ("count".into(), Answer::Number(1.0)),
        ("Old wording".into(), Answer::Legacy(json!("yes"))),
        ("Heading".into(), Answer::Legacy(json!("odd"))),
    ]
    .into_iter()
    .collect();

    let unmatched: Vec<&str> = form
        .unmatched(&answers)
        .map(|(key, _)| key.as_str())
        .collect();

    check!(unmatched == ["Heading", "Old wording"]);
}

#[test]
fn the_latest_date_is_ahead_of_everywhere() {
    let late_utc: Timestamp = "2026-11-11T11:00:00Z".parse().unwrap();

    check!(latest_date(late_utc) == civil::date(2026, 11, 12));
}

#[test]
fn a_form_that_does_not_fit_together_says_why() {
    let consistent = {
        let mut definition = form(vec![
            field("count", number(None, None, true)),
            field("start", time(None)),
            field("end", time(Some("start"))),
            shown_if(field("why", text()), "count", "3"),
        ]);
        definition.repetitions_field = Some("count".into());
        definition.map = Some(FormMap {
            intro: String::new(),
            popup_fields: vec!["why".into()],
        });
        definition
    };
    check!(consistent.inconsistency() == None);

    let twice = form(vec![field("a", text()), field("a", text())]);
    check!(twice.inconsistency().is_some());

    let shown_by_later = form(vec![
        shown_if(field("why", text()), "count", "3"),
        field("count", number(None, None, true)),
    ]);
    check!(shown_by_later.inconsistency().is_some());

    let after_a_number = form(vec![
        field("count", number(None, None, true)),
        field("end", time(Some("count"))),
    ]);
    check!(after_a_number.inconsistency().is_some());

    let mut counted_by_text = form(vec![field("why", text())]);
    counted_by_text.repetitions_field = Some("why".into());
    check!(counted_by_text.inconsistency().is_some());

    let mut mapping_a_heading = form(vec![field("title", FieldKind::Heading)]);
    mapping_a_heading.map = Some(FormMap {
        intro: String::new(),
        popup_fields: vec!["title".into()],
    });
    check!(mapping_a_heading.inconsistency().is_some());
}

#[test]
fn an_answer_shown_again_reads_back_as_itself() {
    let today = civil::date(2026, 11, 11);
    let fields = [
        field("count", number(None, None, false)),
        field("trees", choice(&["oak", "elm"], true)),
        field(
            "seen",
            FieldKind::Boolean {
                style: BooleanStyle::YesNo,
            },
        ),
        field(
            "on",
            FieldKind::Date {
                not_in_future: false,
            },
        ),
        field("at", time(None)),
        field("when", FieldKind::DateTime),
        field("where", FieldKind::Location),
        field("photos", FieldKind::Photo { multiple: true }),
        field("notes", text()),
    ];
    let definition = form(fields.to_vec());
    let raw = raw(&[
        ("count", "2.5"),
        ("trees", "oak"),
        ("trees", "elm"),
        ("seen", "false"),
        ("on", "2026-11-10"),
        ("at", "07:30"),
        ("when", "2026-11-10T07:30"),
        ("where", "38.5,-121.75"),
        ("photos", "https://example.org/a.jpg"),
        ("photos", "https://example.org/b.jpg"),
        ("notes", "Busy"),
    ]);
    let answers = definition.read_submission(&raw, today).unwrap();

    let mut shown = RawAnswers::new();
    for field in &fields {
        shown.insert(
            field.key.to_string(),
            field.control_values(&answers[&field.key]),
        );
    }

    check!(definition.read_submission(&shown, today).unwrap() == answers);
    check!(
        fields[0]
            .control_values(&Answer::Legacy(json!("x")))
            .is_empty()
    );
}

#[test]
fn a_definition_survives_storage_and_the_wire() {
    let mut definition = form(vec![
        required(field("trees", choice(&["0", "1"], false))),
        shown_if(
            field(
                "other",
                FieldKind::Text {
                    multiline: true,
                    placeholder: "e.g.".to_string(),
                },
            ),
            "trees",
            "1",
        ),
    ]);
    definition.map = Some(FormMap {
        intro: "Squirrels".to_string(),
        popup_fields: vec!["trees".into()],
    });

    let json = serde_json::to_string(&definition.fields).unwrap();
    check!(serde_json::from_str::<Vec<FormField>>(&json).unwrap() == definition.fields);

    let wire = flexbuffers::to_vec(&definition).unwrap();
    check!(flexbuffers::from_slice::<FormDefinition>(&wire).unwrap() == definition);

    let answers: Answers = [
        ("date".into(), Answer::Date(civil::date(2026, 1, 2))),
        ("raw".into(), Answer::Legacy(json!({"a": [1, "b"]}))),
    ]
    .into_iter()
    .collect();
    let json = serde_json::to_string(&answers).unwrap();
    check!(serde_json::from_str::<Answers>(&json).unwrap() == answers);
}
