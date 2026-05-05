use std::sync::Arc;

use anyhow::Error;
use common::message::dashboard::{EntitySummary, EntitySummaryKind};
use common::message::user::{BasicInfo, PermissionListRequest, PermissionListResponse};
use poem::{Response, handler, session::Session, web::RealIp};
use sailfish::TemplateOnce;

use crate::component::{Component, Head, StandardLayout};
use crate::dashboard::bookmarks::EntityCardModel;
use crate::dashboard::entity_summary;
use crate::middleware::authorization::Authenticated;

#[handler]
pub async fn page(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;

    let mut all_ids: Vec<String> = Vec::new();
    for resource_type in ["project", "event"] {
        let response: PermissionListResponse = common::service::request(
            "user.permissions.list",
            PermissionListRequest {
                user_id: user_info.user_id.to_string(),
                resource_type: Some(resource_type.to_string()),
            },
        )
        .await?;
        all_ids.extend(response.permissions.into_iter().map(|p| p.resource_id));
    }

    let summaries = entity_summary::lookup_batch(&all_ids).await?;

    render_owned(user_info, summaries).await
}

async fn render_owned(
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

    let content = OwnedContent { projects, events };

    let layout = StandardLayout::new("/dashboard/owned", content)
        .prepare(Some(user_info))
        .await?;

    OwnedPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/owned.stpl.html")]
struct OwnedPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<OwnedContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/owned_content.stpl.html")]
struct OwnedContent {
    projects: Vec<EntityCardModel>,
    events: Vec<EntityCardModel>,
}
