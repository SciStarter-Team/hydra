use sailfish::TemplateOnce;

/// The htmx script tag and its configuration meta tag.
///
/// Every page that uses htmx includes this, so that the configuration lives in
/// exactly one place. [`super::head::Head`] renders it for the main site; the
/// exchange and evolveme templates, which maintain their own `<head>`, include
/// it directly.
#[derive(Debug, Default, TemplateOnce)]
#[template(path = "site/htmx_runtime.stpl.html")]
pub struct HtmxRuntime {
    /// Load the SSE extension alongside htmx.
    pub enable_sse: bool,
}

impl HtmxRuntime {
    #[tracing::instrument]
    pub fn with_sse(mut self) -> Self {
        self.enable_sse = true;
        self
    }
}
