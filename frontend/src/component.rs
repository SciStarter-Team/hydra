pub mod admin_layout;
pub mod detail_page;
pub mod detail_page_edit;
pub mod footer;
pub mod forum;
pub mod head;
pub mod header;
pub mod image_field;
pub mod layout;
pub mod sidebar;
pub mod wordpress;

use poem::{IntoResponse, Response, web::Html};
use sailfish::TemplateOnce;

pub use admin_layout::AdminLayout;
pub use footer::Footer;
pub use forum::{Forum, TopicDetail};
pub use head::Head;
pub use header::Header;
pub use layout::StandardLayout;
pub use sidebar::Sidebar;
pub use wordpress::WordPressContent;

/// Escape text for interpolation into HTML built outside the template
/// system. Templates escape automatically via `<%= %>`; this is for the few
/// places that assemble markup in Rust.
pub fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Wrapper trait for Sailfish's TemplateOnce, enabling an async
/// preparation phase prior to rendering.
#[allow(async_fn_in_trait)]
pub trait Component: Sized {
    type Param;
    type Prepared: TemplateOnce;

    /// Prepare can do any needed asynchronous preparation. If there
    /// are nested components, the prepare implementation should also
    /// call their prepare methods and store the results in the
    /// returned TemplateOnce value fur use during rendering.
    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error>;

    async fn into_string(self, param: Self::Param) -> Result<String, anyhow::Error> {
        Ok(self.prepare(param).await?.render_once()?)
    }

    async fn into_response(self, param: Self::Param) -> Result<Response, anyhow::Error> {
        Ok(Html(self.prepare(param).await?.render_once()?).into_response())
    }
}

/// Temporary "can never be constructed" type, replace with ! once the
/// ! type is stabilized.
pub enum Impossible {}

/// Anything that is TemplateOnce is also trivially a component
impl<T> Component for T
where
    T: TemplateOnce,
{
    type Param = Option<Impossible>;
    type Prepared = T;

    async fn prepare(self, _param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        Ok(self)
    }

    // Optimization: We can skip calling prepare, since a TemplateOnce
    // must by definition be able to render without asynchronous
    // processing.
    async fn into_response(self, _param: Self::Param) -> Result<Response, anyhow::Error> {
        Ok(Html(self.render_once()?).into_response())
    }
}
