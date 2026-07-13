use std::sync::Arc;

use anyhow::Error;
use common::message::dashboard::{EntitySummary, EntitySummaryKind};
use common::message::user::BasicInfo;
use poem::{Response, handler, session::Session, web::RealIp};
use sailfish::TemplateOnce;

use crate::component::{Component, Head, StandardLayout};
use crate::dashboard::entity_summary::{self, entity_link};
use crate::middleware::authorization::Authenticated;

#[handler]
pub async fn page(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;

    let project_ids: Vec<String> =
        common::service::request("user.bookmarks.list_by_user", user_info.user_id.to_string())
            .await?;

    let summaries = entity_summary::lookup_batch(&project_ids).await?;

    render_bookmarks(user_info, summaries).await
}

async fn render_bookmarks(
    user_info: Arc<BasicInfo>,
    summaries: Vec<EntitySummary>,
) -> Result<Response, Error> {
    let projects: Vec<EntityCardModel> = summaries
        .iter()
        .filter(|s| s.kind == EntitySummaryKind::Project)
        .map(EntityCardModel::from_summary)
        .collect();
    let events: Vec<EntityCardModel> = summaries
        .iter()
        .filter(|s| s.kind == EntitySummaryKind::Event)
        .map(EntityCardModel::from_summary)
        .collect();

    let content = BookmarksContent { projects, events };

    let layout = StandardLayout::new("/dashboard/bookmarks", content)
        .prepare(Some(user_info))
        .await?;

    BookmarksPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

pub struct EntityCardModel {
    /// The entity ULID (string form), used to build management links such
    /// as the owned-entity message inbox.
    pub id: String,
    pub name: String,
    pub link: String,
    pub image: Option<String>,
    pub status: String,
}

impl EntityCardModel {
    pub fn from_summary(summary: &EntitySummary) -> Self {
        Self {
            id: summary.id.to_string(),
            name: summary.name.clone(),
            link: entity_link(summary),
            image: summary.image.clone(),
            status: summary.status.clone(),
        }
    }
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/bookmarks.stpl.html")]
struct BookmarksPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<BookmarksContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/bookmarks_content.stpl.html")]
struct BookmarksContent {
    projects: Vec<EntityCardModel>,
    events: Vec<EntityCardModel>,
}
