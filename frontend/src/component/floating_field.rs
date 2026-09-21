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
    icon: Option<FieldIcon>,
    autocomplete: Option<&'static str>,
    description_id: Option<&'static str>,
    invalid: bool,
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
            icon: None,
            autocomplete: None,
            description_id: None,
            invalid: false,
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
    pub fn described_by(mut self, id: &'static str) -> Self {
        self.description_id = Some(id);
        self
    }

    /// Mark the field invalid when `error` is about it.
    pub fn errors(mut self, error: Option<&FormError>) -> Self {
        self.invalid = error.is_some_and(|e| e.is_about(&self.name));
        self
    }

    /// The ids the control's `aria-describedby` lists: the form's error
    /// message when that is about this field, then its guidance.
    fn description_ids(&self) -> String {
        let error = self.invalid.then_some(FormError::ELEMENT_ID);
        error
            .into_iter()
            .chain(self.description_id)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Why a submitted form was refused: a message for the visitor and, where the
/// problem is one control's, that control's `name`. The form shows the message
/// at its top (element id [`FormError::ELEMENT_ID`], focused on load) and marks
/// the control `aria-invalid`, described by the message.
#[derive(Debug, Clone)]
pub struct FormError {
    pub message: String,
    field: Option<String>,
}

impl FormError {
    pub const ELEMENT_ID: &'static str = "form-error";

    /// An error about the control named `field`.
    pub fn at(field: impl Into<String>, message: impl Into<String>) -> Self {
        FormError {
            message: message.into(),
            field: Some(field.into()),
        }
    }

    /// An error about the submission as a whole.
    pub fn general(message: impl Into<String>) -> Self {
        FormError {
            message: message.into(),
            field: None,
        }
    }

    /// Whether this error is about the control named `field`.
    pub fn is_about(&self, field: &str) -> bool {
        self.field.as_deref() == Some(field)
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

/// Where a refused form says why: the message at the top of the form, focused
/// when the page loads so that it is the first thing read and the first thing
/// seen. (Focus, not `role="alert"`: an alert present at load isn't reliably
/// announced, and one that is would be read twice once focused.)
#[derive(TemplateOnce)]
#[template(path = "site/form_error_notice.stpl.html")]
pub struct FormErrorNotice<'a> {
    pub error: Option<&'a FormError>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A field the form's error is about is marked invalid and described by
    /// the error message ahead of its own guidance; other fields are not.
    #[test]
    fn error_marks_only_its_field() {
        let error = FormError::at("name", "Please enter a name.");

        let named = FloatingField::new(FieldKind::Text, "name", "Name", "")
            .described_by("name-help")
            .errors(Some(&error))
            .render_once()
            .unwrap();
        assert!(named.contains(r#"aria-invalid="true""#));
        assert!(named.contains(r#"aria-describedby="form-error name-help""#));

        let other = FloatingField::new(FieldKind::Text, "description", "Details", "")
            .errors(Some(&error))
            .render_once()
            .unwrap();
        assert!(!other.contains("aria-invalid"));
        assert!(!other.contains("aria-describedby"));
    }

    #[test]
    fn general_errors_mark_no_field() {
        let error = FormError::general("Something went wrong.");
        let field = FloatingField::new(FieldKind::Text, "name", "Name", "")
            .errors(Some(&error))
            .render_once()
            .unwrap();
        assert!(!field.contains("aria-invalid"));
    }
}
