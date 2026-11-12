use std::sync::Arc;

use assert2::check;
use common::{
    message::{
        form::{
            BooleanStyle, ChoiceOption, ChoiceWidget, Condition, FieldKind, FormDefinition,
            FormField,
        },
        user::BasicInfo,
    },
    timestring::TimeString,
};
use ulid::Ulid;

use super::*;

/// A field of the kind `kind`, keyed `key`, asking nothing more.
pub(super) fn field(key: &str, label: &str, kind: FieldKind) -> FormField {
    FormField {
        key: key.into(),
        label: label.to_string(),
        help: String::new(),
        required: false,
        visible_if: None,
        remember: false,
        default: None,
        kind,
    }
}

/// A form asking whether a squirrel was seen, which kind if it was, and how
/// many, with a heading that asks nothing.
pub(super) fn form() -> FormDefinition {
    let mut kind = field(
        "kind",
        "Which kind?",
        FieldKind::Choice {
            options: vec![
                ChoiceOption {
                    value: "gray".to_string(),
                    label: "Gray squirrel".to_string(),
                },
                ChoiceOption {
                    value: "red".to_string(),
                    label: "Red squirrel".to_string(),
                },
            ],
            multiple: false,
            widget: ChoiceWidget::Buttons,
        },
    );
    kind.visible_if = Some(Condition {
        field: "seen".into(),
        equals: "true".to_string(),
    });

    FormDefinition {
        form_id: Ulid::new(),
        orrery_id: None,
        slug: Some("squirrels".to_string()),
        name: "Squirrels".to_string(),
        intro: String::new(),
        aside: String::new(),
        image: String::new(),
        fields: vec![
            field("about", "About you", FieldKind::Heading),
            field(
                "seen",
                "Did you see a squirrel?",
                FieldKind::Boolean {
                    style: BooleanStyle::Checkbox,
                },
            ),
            kind,
            field(
                "count",
                "How many?",
                FieldKind::Number {
                    min: Some(0.0),
                    max: None,
                    integer: true,
                },
            ),
        ],
        closed: false,
        sign_in_required: false,
        anonymized: false,
        credits_participation: false,
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

pub(super) fn person(is_staff: bool) -> Arc<BasicInfo> {
    Arc::new(BasicInfo {
        user_id: Ulid::new(),
        handle: "someone".to_string(),
        is_staff,
        is_superuser: false,
        public: true,
        protected: false,
        needs_point: false,
    })
}

#[test]
fn only_a_path_on_this_site_is_somewhere_to_go_next() {
    check!(site_path(" /projects/squirrels ") == Some("/projects/squirrels"));
    check!(site_path("//example.org/") == None);
    check!(site_path("/\\example.org/") == None);
    check!(site_path("https://example.org/") == None);
}

#[test]
fn a_link_is_a_path_here_or_an_address_on_the_web() {
    check!(link("/-uploaded/x") == Some("/-uploaded/x"));
    check!(link("HTTPS://example.org/a.png") == Some("HTTPS://example.org/a.png"));
    check!(link("javascript:alert(1)") == None);
    check!(link("data:image/png;base64,AAAA") == None);
}

#[tokio::test]
async fn staff_and_owners_see_submissions_and_no_one_else_does() {
    let mut form = form();
    let owner = person(false);
    form.owners.push(owner.user_id);

    check!(
        may_see_submissions(&form, Some(&person(true)))
            .await
            .unwrap()
    );
    check!(may_see_submissions(&form, Some(&owner)).await.unwrap());
    check!(
        !may_see_submissions(&form, Some(&person(false)))
            .await
            .unwrap()
    );
    check!(!may_see_submissions(&form, None).await.unwrap());
}
