use assert2::check;
use common::{message::form::Answers, timestring::TimeString};

use super::super::tests::form;
use super::*;

fn record(answers: Answers) -> SubmissionRecord {
    SubmissionRecord {
        submission_id: Ulid::new(),
        orrery_id: Some(17),
        submitter: Some(Ulid::new()),
        when: TimeString::now(),
        answers,
        location: Some(Point {
            latitude: 1.5,
            longitude: -2.25,
        }),
        ip_address: Some("192.0.2.1".to_string()),
        ip_location: None,
        evaluation: Some(Verdict::Negative),
        comment: "Blurry".to_string(),
    }
}

fn answers() -> Answers {
    [
        ("seen".into(), Answer::Bool(true)),
        ("kind".into(), Answer::Choice("red".to_string())),
        ("colour".into(), Answer::Text("ginger".to_string())),
    ]
    .into_iter()
    .collect()
}

#[test]
fn every_row_has_a_value_for_each_column() {
    let form = form();
    let record = record(answers());

    for staff in [false, true] {
        let header = csv_header(&form, staff);
        let row = csv_row(&form, &record, Some("ada@example.org"), staff);

        check!(row.len() == header.len());
    }
}

#[test]
fn only_staff_see_who_and_where_from() {
    let form = form();
    let record = record(answers());

    let header = csv_header(&form, false);
    let row = csv_row(&form, &record, Some("ada@example.org"), false);
    check!(!header.contains(&"submitter_email".to_string()));
    check!(!header.contains(&"ip_address".to_string()));
    check!(!row.contains(&"ada@example.org".to_string()));
    check!(!row.contains(&"192.0.2.1".to_string()));

    let header = csv_header(&form, true);
    let row = csv_row(&form, &record, Some("ada@example.org"), true);
    let column = |name: &str| header.iter().position(|column| column == name).unwrap();
    check!(row[column("submitter_email")] == "ada@example.org");
    check!(row[column("ip_address")] == "192.0.2.1");
}

#[test]
fn a_row_holds_each_answer_under_its_question_and_the_rest_together() {
    let form = form();
    let record = record(answers());
    let header = csv_header(&form, false);
    let row = csv_row(&form, &record, None, false);
    let column = |name: &str| header.iter().position(|column| column == name).unwrap();

    // The heading asks nothing, so has no column.
    check!(!header.contains(&"About you".to_string()));
    check!(row[column("Did you see a squirrel?")] == "true");
    check!(row[column("Which kind?")] == "red");
    check!(row[column("How many?")] == "");
    check!(row[column("other_answers")] == "colour: ginger");
    check!(row[column("latitude")] == "1.5");
    check!(row[column("longitude")] == "-2.25");
    check!(row[column("ip_latitude")] == "");
    check!(row[column("evaluation")] == "negative");
    check!(row[column("comment")] == "Blurry");
}

#[test]
fn a_line_of_the_csv_is_quoted_as_it_needs() {
    let line = csv_line(&[
        "a, b".to_string(),
        "say \"hi\"".to_string(),
        "c".to_string(),
    ])
    .unwrap();

    check!(line == "\"a, b\",\"say \"\"hi\"\"\",c\r\n");
}

#[test]
fn pages_of_submissions_lead_both_ways_while_there_are_more() {
    let content = |offset: u32, shown: usize, total: u32| SubmissionsContent {
        form: form(),
        form_path: "/form/squirrels".to_string(),
        cards: (0..shown)
            .map(|_| SubmissionCard::new(&form(), &record(Answers::new()), None, false))
            .collect(),
        total,
        offset,
    };

    let first = content(0, 25, 60);
    check!(first.newer() == None);
    check!(first.older() == Some(25));

    let last = content(50, 10, 60);
    check!(last.newer() == Some(25));
    check!(last.older() == None);
}
