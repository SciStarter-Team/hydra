pub mod head;
pub mod header;

use poem::{IntoResponse, Response, web::Html};
use sailfish::TemplateOnce;

pub use head::Head;

/// Wrapper trait for Sailfish's TemplateOnce, enabling an async
/// preparation phase prior to rendering.
#[allow(async_fn_in_trait)]
pub trait Component: Sized {
    type Param<'l>;
    type Prepared: TemplateOnce;

    /// Prepare can do any needed asynchronous preparation. If there
    /// are nested components, the prepare implementation should also
    /// call their prepare methods and store the results in the
    /// returned TemplateOnce value fur use during rendering.
    async fn prepare<'l>(self, param: Self::Param<'l>) -> Result<Self::Prepared, anyhow::Error>;

    async fn render<'l>(self, param: Self::Param<'l>) -> Result<Response, anyhow::Error> {
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
    type Param<'l> = Option<Impossible>;
    type Prepared = T;

    async fn prepare<'l>(self, _param: Self::Param<'l>) -> Result<Self::Prepared, anyhow::Error> {
        Ok(self)
    }

    // Optimization: We can skip calling prepare, since a TemplateOnce
    // must by definition be able to render without asynchronous
    // processing.
    async fn render<'l>(self, _param: Self::Param<'l>) -> Result<Response, anyhow::Error> {
        Ok(Html(self.render_once()?).into_response())
    }
}
