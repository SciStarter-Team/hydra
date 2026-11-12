//! Forms SciStarter hosts for projects, and what people submit through them.
//!
//! A form is a list of [`FormField`]s. Most ask a question, and a submission
//! holds one [`Answer`] per question answered, keyed by the field's
//! [`FieldKey`]. Some only show something: a heading, some text, a picture.
//!
//! Everything a submission is held to is checked here, by
//! [`FormDefinition::read_submission`], on the server, whatever the browser
//! did or did not check.
//!
//! Until cutover the forms are copies of orrery's, and so are their
//! submissions. [`FormField::legacy_answer`] reads an answer the way orrery
//! stored it.

use std::collections::BTreeMap;

use jiff::{Timestamp, civil};
use ulid::Ulid;

use crate::message::location::Point;
use crate::timestring::TimeString;

/// The name a submission keeps a field's answer under. Fixed when the field
/// is made, so rewording the field's label does not part it from the
/// answers already given.
///
/// For a form copied from orrery the key is what orrery keyed the answer by:
/// the question as worded, for a form built in orrery's form builder, or the
/// name the project's own page used, for a form that had one.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct FieldKey(pub String);

impl FieldKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for FieldKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::borrow::Borrow<str> for FieldKey {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl From<&str> for FieldKey {
    fn from(key: &str) -> Self {
        FieldKey(key.to_string())
    }
}

/// A form, and what it asks.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormDefinition {
    pub form_id: Ulid,
    /// orrery's id for the form, which old `/form/<id>` links name.
    pub orrery_id: Option<i64>,
    pub slug: Option<String>,
    pub name: String,
    /// Markdown shown above the fields.
    pub intro: String,
    /// Markdown shown beside the form: who runs the project, pictures of
    /// what to look for.
    pub aside: String,
    pub image: String,
    pub fields: Vec<FormField>,
    /// Whether it has stopped taking submissions.
    pub closed: bool,
    /// Whether only someone signed in may submit it.
    pub sign_in_required: bool,
    /// Whether others are shown its submissions only without who made them
    /// or exactly where.
    pub anonymized: bool,
    /// Whether a submission credits its submitter with taking part in the
    /// project.
    pub credits_participation: bool,
    /// The number field whose answer says how many contributions a
    /// submission is; one, where there is none or it is not answered.
    pub repetitions_field: Option<FieldKey>,
    /// Where to send someone once their submission is saved.
    pub next_url: String,
    /// HTML shown once a submission is saved, in place of the usual thanks.
    pub confirmation_message: String,
    /// The slug of the stored email sent to someone the first time they
    /// submit the form, if one is.
    pub first_submission_email: Option<String>,
    pub project: Option<FormProject>,
    /// Who the form belongs to, beside its project's editors: who may see
    /// its submissions.
    #[serde(default)]
    pub owners: Vec<Ulid>,
    /// The public map of its submissions, if it has one.
    pub map: Option<FormMap>,
    pub created: TimeString,
    pub updated: TimeString,
}

/// The project a form belongs to, and how.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FormProject {
    pub project_id: String,
    pub role: FormProjectRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "database", derive(sqlx::Type))]
#[cfg_attr(feature = "database", sqlx(rename_all = "snake_case"))]
pub enum FormProjectRole {
    /// The form taking part in the project means filling in: where its
    /// page's Participate button leads.
    Main,
    /// Another form the project offers beside its main one.
    Extra,
}

/// A form's public map of where its submissions were made.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormMap {
    /// Markdown shown above the map.
    pub intro: String,
    /// The fields whose answers a submission's marker shows, in order.
    pub popup_fields: Vec<FieldKey>,
}

/// One item of a form.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormField {
    pub key: FieldKey,
    /// The question, or for a display item its heading or caption.
    pub label: String,
    /// Markdown offered beside the question to help answer it.
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub required: bool,
    /// Shown, and so asked, only while another answer is this one.
    #[serde(default)]
    pub visible_if: Option<Condition>,
    /// Whether the form starts out answered as the person last answered it,
    /// for a question whose answer seldom changes between visits.
    #[serde(default)]
    pub remember: bool,
    /// What the field starts out answered with, as its control would send
    /// it: a count that is 0 unless changed, say. Only a starting point; an
    /// answer cleared is no answer.
    #[serde(default)]
    pub default: Option<String>,
    pub kind: FieldKind,
}

/// A field shown only while another field's answer is `equals`: a choice's
/// value, `true` or `false` for a yes-or-no question, or some text.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Condition {
    pub field: FieldKey,
    pub equals: String,
}

/// What a field is, and what it accepts.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FieldKind {
    /// A heading over the fields after it.
    Heading,
    /// Markdown.
    Markdown {
        text: String,
    },
    /// A picture, captioned by the field's label.
    Image {
        url: String,
    },
    Text {
        #[serde(default)]
        multiline: bool,
        #[serde(default)]
        placeholder: String,
    },
    Number {
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
        /// Whether only whole numbers are answers.
        #[serde(default)]
        integer: bool,
    },
    Choice {
        options: Vec<ChoiceOption>,
        /// Whether more than one may be chosen.
        #[serde(default)]
        multiple: bool,
        widget: ChoiceWidget,
    },
    Boolean {
        style: BooleanStyle,
    },
    Date {
        /// Whether a day not yet begun anywhere is refused.
        #[serde(default)]
        not_in_future: bool,
    },
    Time {
        #[serde(default)]
        min: Option<civil::Time>,
        #[serde(default)]
        max: Option<civil::Time>,
        /// Minutes between the times offered, where they are offered as a
        /// list rather than typed.
        #[serde(default)]
        step_minutes: Option<u16>,
        /// A time field this one may not be earlier than: an end after its
        /// start.
        #[serde(default)]
        not_before: Option<FieldKey>,
    },
    DateTime,
    Photo {
        #[serde(default)]
        multiple: bool,
    },
    /// A place picked on a map. The first a submission answers is where the
    /// submission was made.
    Location,
}

impl FieldKind {
    /// Whether the field asks something, rather than only showing it.
    pub fn asks(&self) -> bool {
        !matches!(
            self,
            FieldKind::Heading | FieldKind::Markdown { .. } | FieldKind::Image { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChoiceOption {
    /// What the answer stores.
    pub value: String,
    /// What the person is shown.
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChoiceWidget {
    Select,
    /// Radio buttons, or checkboxes for a choice of several.
    Buttons,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BooleanStyle {
    /// One checkbox, labelled by the question: unticked is no.
    Checkbox,
    /// "Yes" and "No" to choose between, so not choosing is no answer.
    YesNo,
    /// "True" and "False" to choose between.
    TrueFalse,
}

/// One answer to one field.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Text(String),
    Number(f64),
    Bool(bool),
    /// A choice's value.
    Choice(String),
    Choices(Vec<String>),
    Date(civil::Date),
    Time(civil::Time),
    /// A date and time where the person was, which is all a form asks.
    DateTime(civil::DateTime),
    /// An instant. Orrery stored times of day and dates with times as
    /// instants, in a time zone it did not record, so that is all an answer
    /// copied from orrery can say.
    Instant(Timestamp),
    /// The addresses of uploaded photos.
    Photos(Vec<String>),
    Location(Point),
    /// An answer as orrery stored it, where it fits no field of the form or
    /// cannot be read as the field's kind: kept rather than lost.
    Legacy(serde_json::Value),
}

/// A submission's answers, by field.
pub type Answers = BTreeMap<FieldKey, Answer>;

/// What a submission form sent: each control's name to its values, in the
/// order sent.
pub type RawAnswers = BTreeMap<String, Vec<String>>;

/// Why a field's answer was refused.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FieldError {
    pub key: FieldKey,
    pub message: String,
}

impl FormDefinition {
    pub fn field(&self, key: &str) -> Option<&FormField> {
        self.fields.iter().find(|field| field.key.as_str() == key)
    }

    /// Read what a submission form sent into answers, holding each to its
    /// field: refused, with every field's reason, where anything is wrong.
    ///
    /// `today` bounds a date that may not be in the future. The server does
    /// not know where the person is, so it is the date in the latest time
    /// zone there is (see [`latest_date`]).
    pub fn read_submission(
        &self,
        raw: &RawAnswers,
        today: civil::Date,
    ) -> Result<Answers, Vec<FieldError>> {
        let mut answers = Answers::new();
        let mut errors = Vec::new();

        for field in self.fields.iter().filter(|field| field.kind.asks()) {
            let values: Vec<&str> = raw
                .get(field.key.as_str())
                .map(|values| values.iter().map(|value| value.trim()).collect())
                .unwrap_or_default();

            match field.read(&values, today) {
                Ok(Some(answer)) => {
                    answers.insert(field.key.clone(), answer);
                }
                Ok(None) => {}
                Err(message) => errors.push(FieldError {
                    key: field.key.clone(),
                    message,
                }),
            }
        }

        // A hidden field is not asked: its answer is dropped, and it is not
        // required. Fields are read in order and conditions refer to earlier
        // fields, but a condition on a field itself hidden is unmet too, so
        // walk until nothing more is hidden.
        loop {
            let hidden: Vec<FieldKey> = self
                .fields
                .iter()
                .filter(|field| answers.contains_key(&field.key) && !field.shown(&answers))
                .map(|field| field.key.clone())
                .collect();

            if hidden.is_empty() {
                break;
            }

            for key in hidden {
                answers.remove(&key);
            }
        }

        errors.retain(|error| {
            self.field(error.key.as_str())
                .is_some_and(|field| field.shown(&answers))
        });

        for field in &self.fields {
            if field.required
                && field.kind.asks()
                && field.shown(&answers)
                && !answers.contains_key(&field.key)
                && !errors.iter().any(|error| error.key == field.key)
            {
                errors.push(FieldError {
                    key: field.key.clone(),
                    message: "This is required.".to_string(),
                });
            }
        }

        for field in &self.fields {
            if let FieldKind::Time {
                not_before: Some(earlier),
                ..
            } = &field.kind
                && let (Some(Answer::Time(this)), Some(Answer::Time(that))) =
                    (answers.get(&field.key), answers.get(earlier))
                && this < that
            {
                let earlier_label = self
                    .field(earlier.as_str())
                    .map_or(earlier.as_str(), |field| field.label.as_str());

                errors.push(FieldError {
                    key: field.key.clone(),
                    message: format!("This can't be earlier than {earlier_label}."),
                });
            }
        }

        if errors.is_empty() {
            Ok(answers)
        } else {
            errors.sort_by_key(|error| self.fields.iter().position(|field| field.key == error.key));
            Err(errors)
        }
    }

    /// What is wrong with how the form fits together, if anything: two
    /// fields sharing a key, or a field the form names (to show another by,
    /// to count repetitions by, to show on its map, to come after) that it
    /// does not have or that is not of the kind named.
    pub fn inconsistency(&self) -> Option<String> {
        for (i, field) in self.fields.iter().enumerate() {
            if self.fields[..i]
                .iter()
                .any(|earlier| earlier.key == field.key)
            {
                return Some(format!(
                    "Two fields share the key {:?}.",
                    field.key.as_str()
                ));
            }

            if let Some(condition) = &field.visible_if
                && !self.fields[..i]
                    .iter()
                    .any(|earlier| earlier.key == condition.field && earlier.kind.asks())
            {
                return Some(format!(
                    "{:?} is shown by {:?}, which is not a question before it.",
                    field.key.as_str(),
                    condition.field.as_str()
                ));
            }

            if let FieldKind::Time {
                not_before: Some(earlier),
                ..
            } = &field.kind
                && !matches!(
                    self.field(earlier.as_str()).map(|field| &field.kind),
                    Some(FieldKind::Time { .. })
                )
            {
                return Some(format!(
                    "{:?} comes after {:?}, which is not a time field.",
                    field.key.as_str(),
                    earlier.as_str()
                ));
            }
        }

        if let Some(key) = &self.repetitions_field
            && !matches!(
                self.field(key.as_str()).map(|field| &field.kind),
                Some(FieldKind::Number { .. })
            )
        {
            return Some(format!(
                "Repetitions are counted by {:?}, which is not a number field.",
                key.as_str()
            ));
        }

        if let Some(map) = &self.map
            && let Some(key) = map.popup_fields.iter().find(|key| {
                !self
                    .field(key.as_str())
                    .is_some_and(|field| field.kind.asks())
            })
        {
            return Some(format!(
                "The map shows {:?}, which is not a question of the form.",
                key.as_str()
            ));
        }

        None
    }

    /// Where a submission with these answers was made: the first place it
    /// picked.
    pub fn location(&self, answers: &Answers) -> Option<Point> {
        self.fields
            .iter()
            .filter(|field| field.kind == FieldKind::Location)
            .find_map(|field| match answers.get(&field.key) {
                Some(Answer::Location(point)) => Some(*point),
                _ => None,
            })
    }

    /// How many contributions a submission with these answers is.
    pub fn repetitions(&self, answers: &Answers) -> u32 {
        let Some(key) = &self.repetitions_field else {
            return 1;
        };

        match answers.get(key) {
            Some(Answer::Number(n)) if *n >= 1.0 && *n <= f64::from(u32::MAX) => *n as u32,
            _ => 1,
        }
    }

    /// The answers in `answers` that no field of this form asks for: given
    /// to a question since reworded, say, or kept from orrery under a name
    /// no field claims.
    pub fn unmatched<'a>(
        &self,
        answers: &'a Answers,
    ) -> impl Iterator<Item = (&'a FieldKey, &'a Answer)> {
        answers.iter().filter(|(key, _)| {
            !self
                .fields
                .iter()
                .any(|field| field.kind.asks() && &field.key == *key)
        })
    }
}

impl FormField {
    /// Whether the field is shown, given the answers so far.
    pub fn shown(&self, answers: &Answers) -> bool {
        let Some(condition) = &self.visible_if else {
            return true;
        };

        match answers.get(&condition.field) {
            Some(Answer::Choice(value)) | Some(Answer::Text(value)) => *value == condition.equals,
            Some(Answer::Choices(values)) => values.contains(&condition.equals),
            Some(Answer::Bool(value)) => value.to_string() == condition.equals,
            _ => false,
        }
    }

    /// This field's answer among `values`, as sent by its controls and
    /// trimmed; `None` where it was left unanswered.
    fn read(&self, values: &[&str], today: civil::Date) -> Result<Option<Answer>, String> {
        let first = values.iter().copied().find(|value| !value.is_empty());

        match &self.kind {
            FieldKind::Heading | FieldKind::Markdown { .. } | FieldKind::Image { .. } => Ok(None),
            FieldKind::Text { .. } => Ok(first.map(|text| Answer::Text(text.to_string()))),
            FieldKind::Number { min, max, integer } => {
                let Some(text) = first else {
                    return Ok(None);
                };

                let number: f64 = text
                    .parse()
                    .ok()
                    .filter(|n: &f64| n.is_finite())
                    .ok_or_else(|| "This needs to be a number.".to_string())?;

                if *integer && number.fract() != 0.0 {
                    return Err("This needs to be a whole number.".to_string());
                }

                if let Some(min) = min
                    && number < *min
                {
                    return Err(format!("This can't be less than {min}."));
                }

                if let Some(max) = max
                    && number > *max
                {
                    return Err(format!("This can't be more than {max}."));
                }

                Ok(Some(Answer::Number(number)))
            }
            FieldKind::Choice {
                options, multiple, ..
            } => {
                let offered = |value: &str| options.iter().any(|option| option.value == value);
                let mut chosen: Vec<String> = Vec::new();

                for value in values.iter().copied().filter(|value| !value.is_empty()) {
                    if !offered(value) {
                        return Err("Please choose one of the options given.".to_string());
                    }

                    if !chosen.iter().any(|already| already == value) {
                        chosen.push(value.to_string());
                    }
                }

                match (chosen.len(), multiple) {
                    (0, _) => Ok(None),
                    (_, true) => Ok(Some(Answer::Choices(chosen))),
                    (1, false) => Ok(Some(Answer::Choice(chosen.remove(0)))),
                    (_, false) => Err("Please choose only one.".to_string()),
                }
            }
            FieldKind::Boolean { style } => match (style, first) {
                (BooleanStyle::Checkbox, None) => Ok(Some(Answer::Bool(false))),
                (_, None) => Ok(None),
                (_, Some("true")) => Ok(Some(Answer::Bool(true))),
                (_, Some("false")) => Ok(Some(Answer::Bool(false))),
                (_, Some(_)) => Err("Please choose one of the options given.".to_string()),
            },
            FieldKind::Date { not_in_future } => {
                let Some(text) = first else {
                    return Ok(None);
                };

                let date: civil::Date = text
                    .parse()
                    .map_err(|_| "Please give a date.".to_string())?;

                if *not_in_future && date > today {
                    return Err("This can't be in the future.".to_string());
                }

                Ok(Some(Answer::Date(date)))
            }
            FieldKind::Time { min, max, .. } => {
                let Some(text) = first else {
                    return Ok(None);
                };

                let time: civil::Time = text
                    .parse()
                    .map_err(|_| "Please give a time.".to_string())?;

                if min.is_some_and(|min| time < min) || max.is_some_and(|max| time > max) {
                    return Err("Please give a time in the range offered.".to_string());
                }

                Ok(Some(Answer::Time(time)))
            }
            FieldKind::DateTime => {
                let Some(text) = first else {
                    return Ok(None);
                };

                text.parse()
                    .map(|when| Some(Answer::DateTime(when)))
                    .map_err(|_| "Please give a date and time.".to_string())
            }
            FieldKind::Photo { multiple } => {
                let photos: Vec<String> = values
                    .iter()
                    .copied()
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect();

                if photos.iter().any(|url| !url.starts_with("https://")) {
                    return Err("Please upload the photo again.".to_string());
                }

                match (photos.len(), multiple) {
                    (0, _) => Ok(None),
                    (1, false) | (_, true) => Ok(Some(Answer::Photos(photos))),
                    (_, false) => Err("Please upload only one photo.".to_string()),
                }
            }
            FieldKind::Location => {
                let Some(text) = first else {
                    return Ok(None);
                };

                parse_location(text)
                    .map(|point| Some(Answer::Location(point)))
                    .ok_or_else(|| "Please pick a place on the map.".to_string())
            }
        }
    }

    /// What this field's controls send for `answer`, so that a form shows it
    /// as answered: the inverse of how [`FormDefinition::read_submission`]
    /// reads them. Nothing for an answer the controls cannot show, such as
    /// one kept from orrery as it was.
    pub fn control_values(&self, answer: &Answer) -> Vec<String> {
        match answer {
            Answer::Text(text) | Answer::Choice(text) => vec![text.clone()],
            Answer::Number(number) => vec![number.to_string()],
            Answer::Bool(value) => vec![value.to_string()],
            Answer::Choices(values) | Answer::Photos(values) => values.clone(),
            Answer::Date(date) => vec![date.to_string()],
            Answer::Time(time) => vec![time.strftime("%H:%M").to_string()],
            Answer::DateTime(when) => vec![when.strftime("%Y-%m-%dT%H:%M").to_string()],
            Answer::Location(point) => vec![format!("{},{}", point.latitude, point.longitude)],
            Answer::Instant(_) | Answer::Legacy(_) => Vec::new(),
        }
    }

    /// This field's answer as orrery stored it in a submission's
    /// `data_json`; `None` for the ways orrery stored no answer at all.
    ///
    /// A value that cannot be read as the field's kind is kept as it was
    /// ([`Answer::Legacy`]) rather than lost.
    pub fn legacy_answer(&self, value: &serde_json::Value) -> Option<Answer> {
        use serde_json::Value;

        if legacy_is_empty(value) {
            return None;
        }

        let read = match (&self.kind, value) {
            (FieldKind::Heading | FieldKind::Markdown { .. } | FieldKind::Image { .. }, _) => None,
            (FieldKind::Text { .. }, Value::String(text)) => Some(Answer::Text(text.clone())),
            (FieldKind::Text { .. }, Value::Number(n)) => Some(Answer::Text(n.to_string())),
            (FieldKind::Number { .. }, Value::Number(n)) => n.as_f64().map(Answer::Number),
            (FieldKind::Number { .. }, Value::String(text)) => {
                text.trim().parse().ok().map(Answer::Number)
            }
            (
                FieldKind::Choice {
                    options,
                    multiple: false,
                    ..
                },
                value,
            ) => legacy_scalar(value)
                .filter(|value| options.iter().any(|option| option.value == *value))
                .map(Answer::Choice),
            (
                FieldKind::Choice {
                    options,
                    multiple: true,
                    ..
                },
                Value::Array(values),
            ) => values
                .iter()
                .map(|value| {
                    legacy_scalar(value)
                        .filter(|value| options.iter().any(|option| option.value == *value))
                })
                .collect::<Option<Vec<String>>>()
                .map(Answer::Choices),
            (FieldKind::Boolean { .. }, Value::Bool(value)) => Some(Answer::Bool(*value)),
            (FieldKind::Date { .. }, Value::String(text)) => legacy_date(text).map(Answer::Date),
            (FieldKind::Time { .. }, Value::String(text)) => text
                .parse()
                .ok()
                .map(Answer::Time)
                .or_else(|| text.parse().ok().map(Answer::Instant)),
            (FieldKind::DateTime, Value::String(text)) => text.parse().ok().map(Answer::Instant),
            (FieldKind::Photo { .. }, Value::String(url)) => {
                Some(Answer::Photos(vec![legacy_photo_url(url)]))
            }
            (FieldKind::Photo { .. }, Value::Array(urls)) => urls
                .iter()
                .map(|url| url.as_str().map(legacy_photo_url))
                .collect::<Option<Vec<String>>>()
                .map(Answer::Photos),
            (FieldKind::Location, Value::Array(pair)) => match pair.as_slice() {
                [longitude, latitude] => Some(Point {
                    latitude: latitude.as_f64()?,
                    longitude: longitude.as_f64()?,
                })
                .filter(point_in_range)
                .map(Answer::Location),
                _ => None,
            },
            _ => None,
        };

        Some(read.unwrap_or_else(|| Answer::Legacy(value.clone())))
    }
}

/// Whether orrery's stored value is one of the ways it stored no answer: a
/// null, an empty string, an empty list.
pub fn legacy_is_empty(value: &serde_json::Value) -> bool {
    use serde_json::Value;

    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(values) => values.is_empty(),
        Value::Object(fields) => fields.is_empty(),
        Value::Bool(_) | Value::Number(_) => false,
    }
}

/// A choice orrery stored as a string, a number or a boolean, as the string
/// a choice's value is.
fn legacy_scalar(value: &serde_json::Value) -> Option<String> {
    use serde_json::Value;

    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// The day orrery's date picker meant. It stored the local midnight of the
/// day picked, as an instant: the same day in UTC for anywhere west of
/// Greenwich, and the next day's for anywhere east, whose midnight falls
/// before UTC's. Every inhabited zone is within twelve hours of UTC either
/// way, so an instant after noon UTC is the next day's midnight.
fn legacy_date(text: &str) -> Option<civil::Date> {
    if let Ok(date) = text.parse::<civil::Date>() {
        return Some(date);
    }

    let instant: Timestamp = text.parse().ok()?;
    let utc = instant.to_zoned(jiff::tz::TimeZone::UTC).datetime();

    if utc.hour() >= 12 {
        utc.date().tomorrow().ok()
    } else {
        Some(utc.date())
    }
}

/// A photo's address as orrery stored it. Some early uploads were stored
/// as a path on scistarter.com rather than a whole address.
fn legacy_photo_url(url: &str) -> String {
    if url.starts_with('/') {
        format!("https://scistarter.com{url}")
    } else {
        url.to_string()
    }
}

fn point_in_range(point: &Point) -> bool {
    (-90.0..=90.0).contains(&point.latitude) && (-180.0..=180.0).contains(&point.longitude)
}

/// A place as a location control sends it: `latitude,longitude`.
pub fn parse_location(text: &str) -> Option<Point> {
    let (latitude, longitude) = text.split_once(',')?;

    Some(Point {
        latitude: latitude
            .trim()
            .parse()
            .ok()
            .filter(|n: &f64| n.is_finite())?,
        longitude: longitude
            .trim()
            .parse()
            .ok()
            .filter(|n: &f64| n.is_finite())?,
    })
    .filter(point_in_range)
}

/// The latest date there is anywhere: the date in UTC+14, the zone furthest
/// ahead. A date no later than this is not in anyone's future.
pub fn latest_date(now: Timestamp) -> civil::Date {
    let ahead = jiff::tz::TimeZone::fixed(jiff::tz::offset(14));

    now.to_zoned(ahead).date()
}

/// Which form: by hydra's id, by the id orrery gave it, or by its slug
/// (`user.forms.get`, answered with an `Option<FormDefinition>`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FormLookup {
    Id(Ulid),
    Orrery(i64),
    Slug(String),
}

/// What a list of forms says of each (`user.forms.list`, and
/// `user.forms.for_project` asked with a project's id).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FormSummary {
    pub form_id: Ulid,
    pub orrery_id: Option<i64>,
    pub slug: Option<String>,
    pub name: String,
    pub closed: bool,
    pub project: Option<FormProject>,
}

/// How saving a form (`user.forms.save`) came out.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FormSaved {
    Saved,
    /// Another form has the slug.
    SlugTaken,
    /// The project already has a main form.
    MainTaken,
    /// Two fields share a key, or a field refers to one the form lacks.
    Inconsistent(String),
    /// The form is a copy of one of orrery's, which orrery keeps until
    /// cutover (`common::primary::CopyKeeper`): the next copy would undo any
    /// change made here.
    KeptByOrrery,
}

/// How deleting a form (`user.forms.delete`) came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FormDeleted {
    /// The form, and every submission of it, are gone.
    Deleted,
    /// There was no such form.
    Missing,
    /// The form is a copy of one of orrery's, which orrery keeps until
    /// cutover: the next copy would bring the form back, without the
    /// submissions deleted with it.
    KeptByOrrery,
}

/// A submission of a form (`user.forms.submit`, answered with a
/// [`SubmitOutcome`]).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormSubmission {
    pub form_id: Ulid,
    /// Who submitted it, if they were signed in.
    pub submitter: Option<Ulid>,
    pub answers: RawAnswers,
    /// Where the request came from. Kept, with where the address is looked
    /// up to be, apart from any place the submitter picked.
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SubmitOutcome {
    Saved { submission_id: Ulid },
    Refused(Vec<FieldError>),
    NoSuchForm,
    Closed,
    SignInRequired,
}

/// The answers someone gave the last time they submitted a form, to the
/// fields it remembers (`user.forms.remembered`, answered with [`Answers`]).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RememberedRequest {
    pub form_id: Ulid,
    pub user_id: Ulid,
}

/// A page of a form's submissions, newest first
/// (`user.forms.submissions`, answered with a [`SubmissionsPage`]).
///
/// A page holds at most `limit` submissions, and fewer where more would not
/// fit in one message; the next page starts after the last one it holds.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SubmissionsRequest {
    pub form_id: Ulid,
    /// Only those this person made, where given.
    pub submitter: Option<Ulid>,
    /// Only those older than this submission, where given: the last of the
    /// page before, so that reading every page reads each submission once
    /// however many arrive meanwhile.
    #[serde(default)]
    pub after: Option<Ulid>,
    /// How many to pass over, after `after`.
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SubmissionsPage {
    pub submissions: Vec<SubmissionRecord>,
    /// How many there are in all, `after` or not.
    pub total: u32,
}

/// One submission, as kept.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SubmissionRecord {
    pub submission_id: Ulid,
    pub orrery_id: Option<i64>,
    pub submitter: Option<Ulid>,
    pub when: TimeString,
    pub answers: Answers,
    /// The place the submitter picked.
    pub location: Option<Point>,
    pub ip_address: Option<String>,
    pub ip_location: Option<Point>,
    /// What the people running the form judged it to be, if they have.
    pub evaluation: Option<Verdict>,
    pub comment: String,
}

/// What the people running a form judged a submission to be, as orrery's
/// card of a submission asked them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "database", derive(sqlx::Type))]
#[cfg_attr(feature = "database", sqlx(rename_all = "snake_case"))]
pub enum Verdict {
    /// Good data, which found what the form looks for: a zombie fly, say.
    Positive,
    /// Good data, which did not.
    Negative,
    /// Neither: not good data, or not data of the kind asked for.
    Invalid,
}

impl Verdict {
    pub const ALL: [Verdict; 3] = [Verdict::Positive, Verdict::Negative, Verdict::Invalid];

    /// The verdict as it is kept, and as a form control sends it.
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Positive => "positive",
            Verdict::Negative => "negative",
            Verdict::Invalid => "invalid",
        }
    }

    /// What the people judging are shown, as orrery showed them.
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Positive => "Valid and a positive",
            Verdict::Negative => "Valid and a negative",
            Verdict::Invalid => "Other",
        }
    }
}

impl std::str::FromStr for Verdict {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Verdict::ALL
            .into_iter()
            .find(|verdict| verdict.as_str() == text)
            .ok_or_else(|| format!("{text:?} is no verdict"))
    }
}

/// One submission of one form: what a request about a submission names,
/// so that whoever may handle one form's submissions cannot reach
/// another's (`user.forms.delete_submission`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SubmissionOf {
    pub form_id: Ulid,
    pub submission_id: Ulid,
}

/// A judgement of a submission, and a note on it (`user.forms.evaluate`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Evaluation {
    pub submission: SubmissionOf,
    pub evaluation: Option<Verdict>,
    pub comment: String,
}

/// One marker on a form's public map (`user.forms.map`, asked with the
/// form's id).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MapMarker {
    /// Where, made vaguer for a form whose submissions are anonymized.
    pub location: Point,
    pub when: TimeString,
    /// The answers the map shows, and only those.
    pub answers: Answers,
}

#[cfg(test)]
mod tests;
