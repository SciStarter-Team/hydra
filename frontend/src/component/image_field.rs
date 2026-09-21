use sailfish::TemplateOnce;

/// An image URL field with a file-upload control and live preview.
///
/// The upload posts to `/add/upload-image`, which answers with out-of-band
/// swaps addressed to the `add-image`, `add-image-status` and
/// `add-image-preview` ids below. That endpoint and this markup are two halves
/// of one contract, so every form that offers an image upload renders it
/// through this widget rather than restating the ids, and the endpoint answers
/// with [`ImageUploadResult`], which renders the URL input through the same
/// [`ImageUrlInput`] the field does.
#[derive(TemplateOnce)]
#[template(path = "site/image_field.stpl.html")]
pub struct ImageField {
    /// Label for the URL input, e.g. "Image".
    pub label: String,
    pub value: String,
    /// Attribution for the image, where the form collects one. Forms that ask
    /// for a photo of the submitters themselves have no one to credit.
    pub credit: Option<String>,
    pub layout: ImageLayout,
}

/// How an [`ImageField`] presents itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageLayout {
    /// A labelled URL input with the upload control and a small preview
    /// beneath it, in the flow of the form.
    #[default]
    Inline,
    /// A banner-shaped upload area leading the form that shows the image
    /// itself once there is one, with the URL and credit fields collapsed
    /// under it.
    Cover,
}

impl ImageLayout {
    /// The value the upload control sends back as `image_layout`, so the
    /// endpoint can answer with the URL input this layout renders.
    pub fn as_str(self) -> &'static str {
        match self {
            ImageLayout::Inline => "inline",
            ImageLayout::Cover => "cover",
        }
    }
}

impl std::str::FromStr for ImageLayout {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "inline" => Ok(ImageLayout::Inline),
            "cover" => Ok(ImageLayout::Cover),
            other => Err(anyhow::anyhow!("unknown image layout {other:?}")),
        }
    }
}

/// The image URL input itself: rendered in place by [`ImageField`], and
/// out-of-band by [`ImageUploadResult`] to set it to an uploaded file's URL.
#[derive(TemplateOnce)]
#[template(path = "site/image_url_input.stpl.html")]
pub struct ImageUrlInput<'a> {
    pub value: &'a str,
    pub layout: ImageLayout,
    pub oob: bool,
}

/// The response to an upload: a status line for the upload control, plus
/// out-of-band updates of the URL input and the preview.
#[derive(TemplateOnce)]
#[template(path = "site/add_image_uploaded.stpl.html")]
pub struct ImageUploadResult {
    pub url: String,
    pub layout: ImageLayout,
}
