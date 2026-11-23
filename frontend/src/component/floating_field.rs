use std::borrow::Cow;

use sailfish::TemplateOnce;

/// A single-value form field whose label sits inside its box: large, in the
/// place of a placeholder, while the field is empty and unfocused, and small
/// above the value otherwise (the `.pill-field` rules in
/// `components/add-form.css`). Date and time inputs always show a value
/// template, so their labels always sit small.
///
/// Fields with behaviour of their own (the place type-ahead, the time-zone
/// picker, selects) write the same `.pill-field` markup by hand; this covers
/// the plain inputs, which are most of them.
#[derive(TemplateOnce)]
#[template(path = "site/floating_field.stpl.html")]
pub struct FloatingField {
    kind: FieldKind,
    name: String,
    label: String,
    value: String,
    id: Option<String>,
    required: bool,
    /// Whether a required field is, for now, not required: another that will
    /// do as well has been answered. It is still marked, so that the page's
    /// script can require it again, but the mark is hidden while the control
    /// isn't `required`.
    waived: bool,
    icon: Option<FieldIcon>,
    autocomplete: Option<&'static str>,
    description_id: Option<Cow<'static, str>>,
    /// The id of the message telling the field's problem, when it has one.
    problem: Option<String>,
    limit: Option<usize>,
    hint: Option<&'static str>,
}

impl FloatingField {
    pub fn new(
        kind: FieldKind,
        name: impl Into<String>,
        label: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        FloatingField {
            kind,
            name: name.into(),
            label: label.into(),
            value: value.into(),
            id: None,
            required: false,
            waived: false,
            icon: None,
            autocomplete: None,
            description_id: None,
            problem: None,
            limit: None,
            hint: None,
        }
    }

    /// Give the control an id. Fields in rows that are cloned client-side
    /// leave it unset, since a clone would repeat it.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Require the field unless `waived` for now: it is required only while
    /// something else on the page says so (no other field of a group that
    /// any will do for has a value, or an answer asks for it), which the
    /// page's script follows, requiring it again once it does.
    pub fn required_unless(mut self, waived: bool) -> Self {
        self.required = true;
        self.waived = waived;
        self
    }

    pub fn icon(mut self, icon: FieldIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The `autocomplete` token saying what the field asks for ("email",
    /// "tel", ...), so browsers and assistive software can fill it in.
    pub fn autocomplete(mut self, token: &'static str) -> Self {
        self.autocomplete = Some(token);
        self
    }

    /// The id of the element giving guidance about the field (a help line
    /// written beside it), which assistive software reads with the field.
    pub fn described_by(mut self, id: impl Into<Cow<'static, str>>) -> Self {
        self.description_id = Some(id.into());
        self
    }

    /// Count the value's characters (code points) against `limit`, at the
    /// right inside the field's box, turning red past it. The field doesn't
    /// cut the value short: the form refuses one over the limit, and a pasted
    /// value can then be trimmed by hand.
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Show `hint` in the empty field while it has focus, once the label has
    /// moved out of its way: what a value looks like, such as "https://".
    pub fn hint(mut self, hint: &'static str) -> Self {
        self.hint = Some(hint);
        self
    }

    /// The control's placeholder: the hint, or a blank one, since either is
    /// what `:placeholder-shown` keys the label's resting position on.
    fn placeholder(&self) -> &'static str {
        self.hint.unwrap_or(" ")
    }

    /// The count of the value's characters, against the field's limit.
    fn length(&self) -> usize {
        self.value.chars().count()
    }

    /// The id of the field's character count, from the control's id or, for
    /// a field without one, its name.
    fn counter_id(&self) -> String {
        format!("{}-count", self.id.as_deref().unwrap_or(&self.name))
    }

    /// Mark the field invalid when one of `errors` is about it.
    pub fn errors(mut self, errors: &FormErrors) -> Self {
        self.problem = errors.described_by(&self.name);
        self
    }

    fn invalid(&self) -> bool {
        self.problem.is_some()
    }

    /// The ids the control's `aria-describedby` lists: its problem's message
    /// when it has one, then its guidance, then its character count.
    fn description_ids(&self) -> String {
        let counter = self.limit.map(|_| self.counter_id());
        self.problem
            .clone()
            .into_iter()
            .chain(self.description_id.as_deref().map(str::to_string))
            .chain(counter)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// One thing wrong with a submitted form: a message for the visitor and, where
/// the problem is one control's, that control's `name`, with any others it
/// implicates (either of which would fix it). The message is shown in an
/// element of its own ([`FormError::id`]), which describes each control it is
/// about, and those controls are marked `aria-invalid`.
#[derive(Debug, Clone)]
pub struct FormError {
    pub message: String,
    field: Option<String>,
    also: Vec<String>,
}

impl FormError {
    /// The id of the notice at the top of a form listing its problems
    /// ([`FormErrorNotice`]).
    pub const ELEMENT_ID: &'static str = "form-error";

    /// The id of a problem about the submission as a whole.
    const GENERAL_ID: &'static str = "form-problem";

    /// An error about the control named `field`.
    pub fn at(field: impl Into<String>, message: impl Into<String>) -> Self {
        FormError {
            message: message.into(),
            field: Some(field.into()),
            also: Vec::new(),
        }
    }

    /// Implicate the control named `field` as well.
    pub fn also(mut self, field: impl Into<String>) -> Self {
        self.also.push(field.into());
        self
    }

    /// An error about the submission as a whole.
    pub fn general(message: impl Into<String>) -> Self {
        FormError {
            message: message.into(),
            field: None,
            also: Vec::new(),
        }
    }

    /// Whether this error is about the control named `field`: the one it
    /// names, or another it implicates.
    pub fn is_about(&self, field: &str) -> bool {
        self.field.as_deref() == Some(field) || self.also.iter().any(|also| also == field)
    }

    /// Whether this error is about the submission as a whole, no control's.
    pub fn is_general(&self) -> bool {
        self.field.is_none()
    }

    /// The id of the element showing the message, which describes the
    /// controls it is about: named for the control it is told under, so that
    /// it stays the same while the form's rows are renumbered (`-problem`
    /// ends every one, which is how a page's script knows them).
    pub fn id(&self) -> String {
        match &self.field {
            Some(field) => format!("{field}-problem"),
            None => Self::GENERAL_ID.to_string(),
        }
    }
}

/// Everything wrong with a submitted form, in the order its checks find
/// them, which is the order of the page, as orrery's event form finds them.
/// Only a control's first problem is told: one fixed, the next shows.
#[derive(Debug, Clone, Default)]
pub struct FormErrors(Vec<FormError>);

impl FormErrors {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tell `error`, unless the control it is told under has a problem told
    /// already.
    pub fn push(&mut self, error: FormError) {
        let told = error.field.as_deref().is_some_and(|field| {
            self.0
                .iter()
                .any(|told| told.field.as_deref() == Some(field))
        });
        if !told {
            self.0.push(error);
        }
    }

    /// The value `checked` comes to, or `None` once what's wrong with it is
    /// told.
    pub fn take<T>(&mut self, checked: Result<T, FormError>) -> Option<T> {
        checked.map_err(|error| self.push(error)).ok()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, FormError> {
        self.0.iter()
    }

    /// The problem the control named `field` is marked with: the first told
    /// under it or implicating it.
    pub fn about(&self, field: &str) -> Option<&FormError> {
        self.0.iter().find(|error| error.is_about(field))
    }

    /// The id of the message describing the control named `field`, when it
    /// has a problem.
    pub fn described_by(&self, field: &str) -> Option<String> {
        self.about(field).map(FormError::id)
    }

    /// Whether the control named `field` has a problem.
    pub fn is_about(&self, field: &str) -> bool {
        self.about(field).is_some()
    }

    /// The first problem told, as a form that shows one at a time shows it.
    pub fn first(&self) -> Option<&FormError> {
        self.0.first()
    }

    /// `Ok(())` when nothing is wrong, and otherwise everything that is.
    pub fn into_result(self) -> Result<(), FormErrors> {
        if self.is_empty() { Ok(()) } else { Err(self) }
    }
}

impl From<FormError> for FormErrors {
    fn from(error: FormError) -> Self {
        FormErrors(vec![error])
    }
}

impl From<Option<FormError>> for FormErrors {
    fn from(error: Option<FormError>) -> Self {
        FormErrors(error.into_iter().collect())
    }
}

impl<'a> IntoIterator for &'a FormErrors {
    type Item = &'a FormError;
    type IntoIter = std::slice::Iter<'a, FormError>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Email,
    Tel,
    Url,
    Date,
    Time,
    TextArea { rows: u8 },
}

impl FieldKind {
    fn control(self) -> Control {
        match self {
            FieldKind::Text => Control::Input("text"),
            FieldKind::Email => Control::Input("email"),
            FieldKind::Tel => Control::Input("tel"),
            FieldKind::Url => Control::Input("url"),
            FieldKind::Date => Control::Input("date"),
            FieldKind::Time => Control::Input("time"),
            FieldKind::TextArea { rows } => Control::TextArea { rows },
        }
    }
}

/// The element a field renders as: an `<input>` of the given type, or a
/// `<textarea>`.
#[derive(Clone, Copy)]
enum Control {
    Input(&'static str),
    TextArea { rows: u8 },
}

/// The leading icon drawn inside a field's box.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FieldIcon {
    Calendar,
    Clock,
}

/// Where a refused form says why: a notice at the top of the form listing
/// its problems, each a button that takes the visitor to the control it
/// describes, focused when the page loads so that it is the first thing read
/// and the first thing seen. (Focus, not `role="alert"`: an alert present at
/// load isn't reliably announced, and one that is would be read twice once
/// focused.) With more than one problem, it says what they are first, for
/// the `noun` the form adds or saves.
#[derive(TemplateOnce)]
#[template(path = "site/form_error_notice.stpl.html")]
pub struct FormErrorNotice<'a> {
    pub errors: &'a FormErrors,
    pub noun: &'a str,
}

/// A refused change's problems, told in a form that changes one part of a
/// page, each in the element that describes its controls.
#[derive(TemplateOnce)]
#[template(path = "site/form_error_messages.stpl.html")]
pub struct FormErrorMessages<'a> {
    pub errors: &'a FormErrors,
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    /// A field the form's error is about is marked invalid and described by
    /// the error message ahead of its own guidance; other fields are not.
    #[test]
    fn error_marks_only_its_field() {
        let error = FormError::at("name", "Please enter a name.");

        let named = FloatingField::new(FieldKind::Text, "name", "Name", "")
            .described_by("name-help")
            .errors(&FormErrors::from(error.clone()))
            .render_once()
            .unwrap();
        assert!(named.contains(r#"aria-invalid="true""#));
        assert!(named.contains(r#"aria-describedby="name-problem name-help""#));

        let other = FloatingField::new(FieldKind::Text, "description", "Details", "")
            .errors(&FormErrors::from(error.clone()))
            .render_once()
            .unwrap();
        assert!(!other.contains("aria-invalid"));
        assert!(!other.contains("aria-describedby"));
    }

    #[test]
    fn a_limited_field_counts_its_characters_after_its_guidance() {
        let field = |value: &str| {
            FloatingField::new(FieldKind::Text, "name", "Name", value)
                .id("event-name")
                .described_by("name-help")
                .limit(5)
                .render_once()
                .unwrap()
        };

        // Counted in code points, not bytes or UTF-16 units.
        let html = field("Café");
        check!(html.contains(r#"<span id="event-name-count" class="pill-counter">4 / 5</span>"#));
        check!(html.contains(r#"aria-describedby="name-help event-name-count""#));
        check!(html.contains("hx-on:input="));

        let html = field("Cafés!");
        check!(html.contains(r#"class="pill-counter pill-counter-over">6 / 5</span>"#));
        check!(html.contains(r#"value="Cafés!""#));
    }

    #[test]
    fn general_errors_mark_no_field() {
        let error = FormError::general("Something went wrong.");
        let field = FloatingField::new(FieldKind::Text, "name", "Name", "")
            .errors(&FormErrors::from(error.clone()))
            .render_once()
            .unwrap();
        assert!(!field.contains("aria-invalid"));
    }

    /// Every problem is told, in the order found, but only a control's
    /// first; a control another implicates is marked with that one.
    #[test]
    fn a_control_has_its_first_problem_told() {
        let mut errors = FormErrors::new();
        errors.push(FormError::at("name", "Please enter a name."));
        errors.push(FormError::at("how_to_join", "Please explain how.").also("url"));
        errors.push(FormError::at("name", "Please shorten the name."));
        errors.push(FormError::at("url", "Please check the webpage."));

        let told = errors
            .iter()
            .map(|e| e.message.as_str())
            .collect::<Vec<_>>();
        check!(
            told == [
                "Please enter a name.",
                "Please explain how.",
                "Please check the webpage."
            ]
        );
        check!(errors.described_by("url") == Some("how_to_join-problem".to_string()));
        check!(errors.described_by("description") == None);
    }

    /// The notice lists the problems, each in the element that describes
    /// its control, saying what they are first only when there are several.
    #[test]
    fn the_notice_lists_every_problem() {
        let notice = |errors: &FormErrors| {
            FormErrorNotice {
                errors,
                noun: "event",
            }
            .render_once()
            .unwrap()
        };
        check!(notice(&FormErrors::new()).trim().is_empty());

        let mut errors = FormErrors::from(FormError::at("name", "Please enter a name."));
        let one = notice(&errors);
        check!(one.contains(
            r#"<li id="name-problem"><button type="button">Please enter a name.</button></li>"#
        ));
        check!(!one.contains("Please fix these"));

        errors.push(FormError::general("Something went wrong."));
        let two = notice(&errors);
        check!(two.contains("Please fix these before submitting your event:"));
        check!(two.contains(r#"<li id="form-problem">Something went wrong.</li>"#));
    }
}
