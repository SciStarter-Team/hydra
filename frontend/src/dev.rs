use std::sync::Arc;

use poem::{Endpoint, EndpointExt, Route, handler};
use sailfish::TemplateOnce;

use crate::middleware::authorization::StaffOnly;
use crate::{
    component::{Component, Footer, Head, Header},
    middleware::authorization::Authenticated,
};
use common::message::user::BasicInfo;

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/background-processing", background_processing)
        .at("/translation", translation)
        .with(StaffOnly)
}

#[handler]
#[tracing::instrument]
pub async fn background_processing(
    Authenticated(basic_info): Authenticated,
) -> poem::Result<poem::Response> {
    Ok(BackgroundProcessing.into_response(basic_info).await?)
}

#[derive(Debug)]
pub struct BackgroundProcessing;

impl Component for BackgroundProcessing {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = BackgroundProcessingTpl;

    #[tracing::instrument]
    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let header = Header::default().prepare(param.clone()).await?;
        let footer = Footer::default().prepare(param).await?;
        Ok(BackgroundProcessingTpl { header, footer })
    }
}

#[derive(TemplateOnce)]
#[template(path = "dev/background_processing.stpl.html")]
pub struct BackgroundProcessingTpl {
    header: <Header as Component>::Prepared,
    footer: <Footer as Component>::Prepared,
}

/// A worked example of translation, and the proof that a template can do it.
///
/// Every phrase on the page is translated by the template itself during
/// rendering, which is only possible because [`crate::i18n::tr`] needs
/// neither the database nor an await. Visiting it in another language is
/// enough to stock the phrases for a translator.
#[handler]
#[tracing::instrument]
pub async fn translation(Authenticated(basic_info): Authenticated) -> poem::Result<poem::Response> {
    Ok(Translation.into_response(basic_info).await?)
}

#[derive(Debug)]
pub struct Translation;

impl Component for Translation {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = TranslationTpl;

    #[tracing::instrument]
    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        let name = param
            .as_ref()
            .map(|info| info.display_handle().to_string())
            .unwrap_or_else(|| String::from("stranger"));

        let header = Header::default().prepare(param.clone()).await?;
        let footer = Footer::default().prepare(param).await?;

        Ok(TranslationTpl {
            header,
            footer,
            name,
            lang: crate::i18n::current_lang(),
        })
    }
}

#[derive(TemplateOnce)]
#[template(path = "dev/translation.stpl.html")]
pub struct TranslationTpl {
    header: <Header as Component>::Prepared,
    footer: <Footer as Component>::Prepared,
    name: String,
    lang: crate::i18n::Lang,
}
