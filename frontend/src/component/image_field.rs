use sailfish::TemplateOnce;

/// An image URL field with a file-upload control and live preview.
///
/// The upload posts to `/add/upload-image`, which answers with out-of-band
/// swaps addressed to the `add-image`, `add-image-status` and
/// `add-image-preview` ids below. That endpoint and this markup are two halves
/// of one contract, so every form that offers an image upload renders it
/// through this widget rather than restating the ids.
#[derive(TemplateOnce)]
#[template(path = "site/image_field.stpl.html")]
pub struct ImageField {
    /// Label for the URL input, e.g. "Image".
    pub label: String,
    pub value: String,
    /// Attribution for the image, where the form collects one. Forms that ask
    /// for a photo of the submitters themselves have no one to credit.
    pub credit: Option<String>,
}
