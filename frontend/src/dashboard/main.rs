use std::sync::Arc;

use anyhow::{Error, anyhow};
use common::message::dashboard::{EntitySummary, EntitySummaryKind, LinkToken};
use common::message::log::{
    UserParticipationRequest, UserParticipationResponse, UserRecentVisitsRequest,
    UserRecentVisitsResponse,
};
use common::message::user::{BasicInfo, PermissionListRequest, PermissionListResponse};
use poem::{Response, handler, session::Session, web::RealIp};
use sailfish::TemplateOnce;

use crate::component::{Component, Head, StandardLayout};
use crate::dashboard::entity_summary::{self, entity_link};
use crate::middleware::authorization::Authenticated;

const RECENT_VISIT_DAYS: i64 = 30;
const RECENT_VISIT_LIMIT: i64 = 12;

#[handler]
pub async fn page(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let user_id = user_info.user_id;
    let user_id_str = user_id.to_string();

    let participation: UserParticipationResponse = common::service::request(
        "log.user_participation",
        UserParticipationRequest {
            user_id,
            recent_limit: 0,
        },
    )
    .await
    .unwrap_or(UserParticipationResponse {
        total: 0,
        recent: vec![],
    });

    let mut owned_ids: Vec<String> = Vec::new();
    for resource_type in ["project", "event"] {
        let response: PermissionListResponse = common::service::request(
            "user.permissions.list",
            PermissionListRequest {
                user_id: user_id_str.clone(),
                resource_type: Some(resource_type.to_string()),
            },
        )
        .await
        .unwrap_or(PermissionListResponse {
            permissions: vec![],
        });
        owned_ids.extend(response.permissions.into_iter().map(|p| p.resource_id));
    }
    let owned = entity_summary::lookup_batch(&owned_ids).await?;

    let joined_ids: Vec<String> =
        common::service::request("user.joins.list_by_user_id", user_id_str.clone())
            .await
            .unwrap_or_default();
    let joined = entity_summary::lookup_batch(&joined_ids).await?;

    let bookmark_ids: Vec<String> =
        common::service::request("user.bookmarks.list_by_user", user_id_str.clone())
            .await
            .unwrap_or_default();
    let bookmarked = entity_summary::lookup_batch(&bookmark_ids).await?;

    let visits: UserRecentVisitsResponse = common::service::request(
        "log.user_recent_visits",
        UserRecentVisitsRequest {
            user_id,
            days: RECENT_VISIT_DAYS,
            limit: RECENT_VISIT_LIMIT,
        },
    )
    .await
    .unwrap_or(UserRecentVisitsResponse { visits: vec![] });

    let candidate_slugs: Vec<String> = visits
        .visits
        .iter()
        .filter_map(|v| {
            let trimmed = v.path.trim_start_matches('/');
            if trimmed.is_empty() || trimmed.contains('/') || trimmed.contains('?') {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect();
    let visited = entity_summary::lookup_batch_by_slug(&candidate_slugs).await?;

    let link_tokens: Vec<LinkToken> =
        common::service::request("user.link_tokens.list_by_user", user_id_str.clone())
            .await
            .unwrap_or_default();

    render_overview(
        user_info,
        participation.total,
        owned,
        joined,
        bookmarked,
        visited,
        link_tokens,
    )
    .await
}

async fn render_overview(
    user_info: Arc<BasicInfo>,
    total_contributions: i64,
    owned: Vec<EntitySummary>,
    joined: Vec<EntitySummary>,
    bookmarked: Vec<EntitySummary>,
    visited: Vec<EntitySummary>,
    link_tokens: Vec<LinkToken>,
) -> Result<Response, Error> {
    let owned_count = owned.len() as i64;
    let joined_count = joined.len() as i64;
    let bookmarked_count = bookmarked.len() as i64;

    let owned_cards: Vec<EntityCardModel> = owned.iter().map(EntityCardModel::from_summary).collect();
    let joined_cards: Vec<EntityCardModel> =
        joined.iter().map(EntityCardModel::from_summary).collect();
    let bookmarked_cards: Vec<EntityCardModel> = bookmarked
        .iter()
        .map(EntityCardModel::from_summary)
        .collect();
    let visited_cards: Vec<EntityCardModel> =
        visited.iter().map(EntityCardModel::from_summary).collect();

    let token_cards: Vec<LinkTokenCardModel> = link_tokens
        .into_iter()
        .map(LinkTokenCardModel::from)
        .collect();

    let content = OverviewContent {
        first_name: user_info.handle.clone(),
        total_contributions,
        owned_count,
        joined_count,
        bookmarked_count,
        owned: owned_cards,
        joined: joined_cards,
        bookmarked: bookmarked_cards,
        visited: visited_cards,
        link_tokens: token_cards,
    };

    let layout = StandardLayout::new("/dashboard", content)
        .prepare(Some(user_info))
        .await?;

    OverviewPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

pub struct EntityCardModel {
    pub kind: EntitySummaryKind,
    pub name: String,
    pub link: String,
    pub image: Option<String>,
    pub status: String,
}

impl EntityCardModel {
    pub fn from_summary(summary: &EntitySummary) -> Self {
        Self {
            kind: summary.kind,
            name: summary.name.clone(),
            link: entity_link(summary),
            image: summary.image.clone(),
            status: summary.status.clone(),
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            EntitySummaryKind::Project => "Project",
            EntitySummaryKind::Event => "Event",
        }
    }
}

pub struct LinkTokenCardModel {
    pub label: String,
    pub created: String,
    pub expires: Option<String>,
}

impl From<LinkToken> for LinkTokenCardModel {
    fn from(t: LinkToken) -> Self {
        Self {
            label: t.label,
            created: t.created,
            expires: t.expires,
        }
    }
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/main.stpl.html")]
struct OverviewPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<OverviewContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/main_content.stpl.html")]
struct OverviewContent {
    first_name: String,
    total_contributions: i64,
    owned_count: i64,
    joined_count: i64,
    bookmarked_count: i64,
    owned: Vec<EntityCardModel>,
    joined: Vec<EntityCardModel>,
    bookmarked: Vec<EntityCardModel>,
    visited: Vec<EntityCardModel>,
    link_tokens: Vec<LinkTokenCardModel>,
}
