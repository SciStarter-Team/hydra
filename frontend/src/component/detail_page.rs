use std::sync::Arc;
use std::time::Duration;

use anyhow::Error;
use common::{
    message::{
        log::{
            LastEditedRequest, LastEditedResponse, ProjectLikesRequest, ProjectLikesResponse,
            ProjectMemberCountRequest, ProjectMemberCountResponse, ProjectReviewsRequest,
            ProjectReviewsResponse, RatingSetRequest, ReviewEntry, ReviewSetRequest, SharePlatform,
        },
        user::{
            BasicInfo, BookmarkCheckRequest, BookmarkCheckResponse, BookmarkCreateRequest,
            BookmarkCreateResponse, ProjectJoinCheckRequest, ProjectJoinCheckResponse,
            UserDisplayInfoBatchRequest, UserDisplayInfoBatchResponse,
        },
    },
    service::{log_share, request, send},
    shared::db,
    timestring::TimeString,
};
use poem::{
    IntoResponse, handler,
    web::{Html, Path},
};
use sailfish::TemplateOnce;
use std::collections::HashMap;
use ulid::Ulid;

use crate::component::Component;
use crate::event::{EventStatus, EventType};
use crate::lazy_cache::{LazyCached, get_cached};
use crate::local_time::{TimeFormat, local_time};
use crate::middleware::analytics::{Device, Interaction};
use crate::middleware::authorization::Authenticated;
use crate::participant_file::ParticipantFile;
use crate::peering::LoadStorePeered;
use crate::project::ProjectStatus;
use common::quill::content_to_html;

const DOMAIN: &str = env!("DOMAIN");

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailPageKind {
    Project,
    Event,
}

impl DetailPageKind {
    pub fn is_project(self) -> bool {
        matches!(self, Self::Project)
    }

    pub fn is_event(self) -> bool {
        matches!(self, Self::Event)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Event => "event",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailPageStatus {
    Active,
    Finished,
    Hiatus,
    Pending,
    Hidden,
    Cancelled,
}

impl DetailPageStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    pub fn alert_message(&self) -> &'static str {
        match self {
            Self::Active => "",
            Self::Finished => "This project is complete.",
            Self::Hiatus => "This project is on hiatus.",
            Self::Pending => "This project is pending review.",
            Self::Hidden => "This project is not currently visible.",
            Self::Cancelled => "This event has been cancelled.",
        }
    }
}

impl From<ProjectStatus> for DetailPageStatus {
    fn from(s: ProjectStatus) -> Self {
        match s {
            ProjectStatus::Active | ProjectStatus::PeriodicallyActive => Self::Active,
            ProjectStatus::Complete | ProjectStatus::Expired => Self::Finished,
            ProjectStatus::Hiatus => Self::Hiatus,
            ProjectStatus::Pending => Self::Pending,
            ProjectStatus::Hidden => Self::Hidden,
        }
    }
}

impl From<EventStatus> for DetailPageStatus {
    fn from(s: EventStatus) -> Self {
        match s {
            EventStatus::Active => Self::Active,
            EventStatus::Cancelled => Self::Cancelled,
            EventStatus::Expired => Self::Finished,
        }
    }
}

// ---------------------------------------------------------------------------
// Display structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct EventInstanceDisplay {
    pub begin: TimeString,
    pub end: TimeString,
}

/// Review enriched with user display information for template rendering.
#[derive(Debug, Clone)]
pub struct ReviewForDisplay {
    pub user_id: String,
    pub user_handle: String,
    pub user_image: String,
    pub text: String,
    pub when: TimeString,
}

// ---------------------------------------------------------------------------
// Lazy-cached participant count
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct ParticipantCount {
    project_id: Ulid,
}

impl LazyCached for ParticipantCount {
    fn cache_key(&self) -> String {
        format!("participant_count:{}", self.project_id)
    }

    fn max_age(&self) -> Duration {
        Duration::from_secs(3600)
    }

    async fn fetch_fresh(&self) -> Result<String, Error> {
        let resp: ProjectMemberCountResponse = request(
            "log.project_member_count",
            ProjectMemberCountRequest {
                project_id: self.project_id.to_string(),
            },
        )
        .await?;
        Ok(resp.count.to_string())
    }
}

#[derive(Clone)]
struct LastEdited {
    target_id: Ulid,
}

impl LazyCached for LastEdited {
    fn cache_key(&self) -> String {
        format!("last_edited:{}", self.target_id)
    }

    fn max_age(&self) -> Duration {
        Duration::from_secs(3600)
    }

    async fn fetch_fresh(&self) -> Result<String, Error> {
        let resp: LastEditedResponse = request(
            "log.last_edited",
            LastEditedRequest {
                target_id: self.target_id.to_string(),
            },
        )
        .await?;
        Ok(resp.when)
    }
}

/// How long the frontend holds a cached per-project like count / review list
/// before a background refresh. The log-service stays authoritative; this just
/// reduces read traffic so the frontends scale horizontally.
const ENGAGEMENT_CACHE_MAX_AGE: Duration = Duration::from_secs(600);

/// Per-project like count, cached on each frontend instance.
#[derive(Clone)]
struct LikeCount {
    target_id: Ulid,
}

impl LazyCached for LikeCount {
    fn cache_key(&self) -> String {
        format!("like_count:{}", self.target_id)
    }

    fn max_age(&self) -> Duration {
        ENGAGEMENT_CACHE_MAX_AGE
    }

    async fn fetch_fresh(&self) -> Result<String, Error> {
        let resp: ProjectLikesResponse = request(
            "log.project_likes",
            ProjectLikesRequest {
                project_id: self.target_id.to_string(),
                user_id: None,
            },
        )
        .await?;
        Ok(resp.count.to_string())
    }
}

/// The cached like count for a target (project or event).
pub(crate) async fn cached_like_count(target_id: Ulid) -> Result<i64, Error> {
    Ok(get_cached(&LikeCount { target_id })
        .await?
        .parse()
        .unwrap_or(0))
}

/// Whether a specific user has liked a target. Always fetched live (per-user
/// state is not shared-cacheable), so only costs a request for logged-in users.
pub(crate) async fn user_has_liked(target_id: Ulid, user_id: Ulid) -> Result<bool, Error> {
    let resp: ProjectLikesResponse = request(
        "log.project_likes",
        ProjectLikesRequest {
            project_id: target_id.to_string(),
            user_id: Some(user_id.to_string()),
        },
    )
    .await?;
    Ok(resp.liked)
}

/// Per-project public reviews (list + count), cached on each frontend instance.
/// We cache the largest list any caller needs and let callers take a prefix.
#[derive(Clone)]
struct ReviewsCache {
    target_id: Ulid,
}

impl LazyCached for ReviewsCache {
    fn cache_key(&self) -> String {
        format!("project_reviews:{}", self.target_id)
    }

    fn max_age(&self) -> Duration {
        ENGAGEMENT_CACHE_MAX_AGE
    }

    async fn fetch_fresh(&self) -> Result<String, Error> {
        let resp: ProjectReviewsResponse = request(
            "log.project_reviews",
            ProjectReviewsRequest {
                project_id: self.target_id.to_string(),
                limit: 10,
            },
        )
        .await?;
        Ok(serde_json::to_string(&resp)?)
    }
}

/// The cached public reviews (list + total public count) for a target.
pub(crate) async fn cached_project_reviews(
    target_id: Ulid,
) -> Result<ProjectReviewsResponse, Error> {
    let json = get_cached(&ReviewsCache { target_id }).await?;
    Ok(
        serde_json::from_str(&json).unwrap_or(ProjectReviewsResponse {
            reviews: Vec::new(),
            count: 0,
        }),
    )
}

// ---------------------------------------------------------------------------
// Unprepared component
// ---------------------------------------------------------------------------

pub struct DetailPage {
    pub slug: String,
    pub kind: DetailPageKind,
    pub target_id: Ulid,
}

// ---------------------------------------------------------------------------
// Prepared struct (template)
// ---------------------------------------------------------------------------

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_content.stpl.html")]
pub struct DetailPageContent {
    pub kind: DetailPageKind,
    pub slug: String,
    pub target_id: String,

    // Hero
    pub name: String,
    pub presenter: String,
    pub image: String,
    pub image_credit: String,
    pub url: String,
    pub status: DetailPageStatus,

    // Engagement
    pub like_count: i64,
    pub user_has_liked: bool,
    pub saved: bool,
    pub joined: bool,

    // Info cards
    pub goal: String,
    pub task: String,
    pub description: String,
    /// Plain-text excerpt of the description, for the og:description share tag.
    pub meta_description: String,
    pub how_to_join: String,

    // Location
    pub location_type: String,
    pub entirely_online: bool,
    pub location_name: String,
    pub location_description: String,
    pub address: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub country: String,
    pub has_point: bool,

    // More Info accordion (project only)
    pub total_expense: String,
    pub time_per_participation: String,
    pub audiences: Vec<String>,
    pub participation_schedule: String,
    pub mobile_app_url: String,
    pub mobile_app_url_ios: String,
    pub mobile_app_url_android: String,
    pub training_materials: String,
    pub classroom_materials: String,
    pub participant_files: Vec<ParticipantFile>,
    pub video_url: String,

    // Social media
    pub twitter_name: String,
    pub facebook_page: String,
    pub instagram_page: String,

    // Tags
    pub tags: Vec<String>,

    // Metadata
    pub last_updated: Option<TimeString>,
    pub member_count: i64,

    // Event-specific
    pub event_type: Option<String>,
    pub instances: Vec<EventInstanceDisplay>,

    // User state
    pub user_info: Option<Arc<BasicInfo>>,

    // Reviews
    pub reviews: Vec<ReviewForDisplay>,
    pub review_count: i64,

    // Editing permissions
    pub can_edit: bool,
    pub is_staff_editor: bool,

    // Additional editable fields (populated only when can_edit)
    pub additional_fields: Vec<super::detail_page_edit::AdditionalField>,

    // Pre-rendered per-field display partials, keyed by field_type. The full page
    // renders each editable field through the same partial as an inline save, so
    // a saved field always matches a full page render.
    pub editable_fields: HashMap<String, super::detail_page_edit::FieldDisplay>,
}

impl DetailPageContent {
    /// The display partial for an editable field, for inclusion via `<%+ %>`.
    pub fn field(&self, field_type: &str) -> super::detail_page_edit::FieldDisplay {
        self.editable_fields
            .get(field_type)
            .cloned()
            .unwrap_or_else(|| {
                super::detail_page_edit::FieldDisplay::build(
                    self.target_id.clone(),
                    field_type,
                    String::new(),
                    self.can_edit,
                    String::new(),
                    true,
                )
            })
    }
}

// ---------------------------------------------------------------------------
// Include structs for template partials
// ---------------------------------------------------------------------------

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_like_button.stpl.html")]
pub struct LikeButtonInclude<'a> {
    pub like_count: i64,
    pub user_has_liked: bool,
    pub target_id: &'a str,
}

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_save_button.stpl.html")]
pub struct SaveButtonInclude<'a> {
    pub saved: bool,
    pub target_id: &'a str,
}

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_review_form.stpl.html")]
pub struct ReviewFormInclude<'a> {
    pub target_id: &'a str,
}

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page_review.stpl.html")]
pub struct ReviewEntryInclude<'a> {
    pub review: &'a ReviewForDisplay,
    pub oob: bool,
}

// ---------------------------------------------------------------------------
// Page wrapper (outermost template)
// ---------------------------------------------------------------------------

#[derive(TemplateOnce)]
#[template(path = "pages/detail_page.stpl.html")]
pub struct DetailPageWrapper {
    pub head: <crate::component::Head as Component>::Prepared,
    pub layout: <crate::component::StandardLayout<DetailPageContent> as Component>::Prepared,
    pub title: String,
    /// Open Graph metadata so social shares use the entity's own uploaded image
    /// and details, rather than scraping the SciStarter logo from the header.
    pub og_title: String,
    pub og_description: String,
    /// Absolute URL of the entity's uploaded image. Empty when none was uploaded.
    pub og_image: String,
    pub og_url: String,
}

// ---------------------------------------------------------------------------
// Component impl
// ---------------------------------------------------------------------------

impl Component for DetailPage {
    type Param = Option<Arc<BasicInfo>>;
    type Prepared = DetailPageContent;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, Error> {
        match self.kind {
            DetailPageKind::Project => self.prepare_project(param).await,
            DetailPageKind::Event => self.prepare_event(param).await,
        }
    }
}

impl DetailPage {
    async fn prepare_project(
        self,
        user_info: Option<Arc<BasicInfo>>,
    ) -> Result<DetailPageContent, Error> {
        let id_str = self.target_id.to_string();

        let row = sqlx::query!(
            r#"
            SELECT
                name,
                status as "status: ProjectStatus",
                presenter,
                goal,
                task,
                description,
                image,
                image_credit,
                url,
                video_url,
                twitter_name,
                facebook_page,
                instagram_page,
                how_to_join,
                location_type,
                entirely_online as "entirely_online: bool",
                location_name,
                location_description,
                address,
                city,
                state,
                zip,
                country,
                total_expense,
                time_per_participation,
                training_materials,
                classroom_materials,
                mobile_app_url,
                mobile_app_url_android,
                mobile_app_url_ios,
                participation_schedule,
                point IS NOT NULL as "has_point: bool",
                created
            FROM projects
            WHERE project_id = ?
            "#,
            id_str
        )
        .fetch_optional(db())
        .await?
        .ok_or_else(|| anyhow::anyhow!("Project not found: {}", self.target_id))?;

        // Concurrent secondary queries
        let project_id = self.target_id;
        let id_str2 = id_str.clone();
        let id_str3 = id_str.clone();
        let id_str4 = id_str.clone();
        let id_str5 = id_str.clone();
        let id_str6 = id_str.clone();
        let user_info_clone = user_info.clone();
        let user_info_clone2 = user_info.clone();

        let (
            tags,
            audiences,
            participant_files,
            likes,
            engagement,
            reviews_and_count,
            member_count,
            last_edited_str,
            edit_permission,
        ) = tokio::try_join!(
            // Tags
            async {
                sqlx::query_scalar!(
                    "SELECT tag FROM project_tags WHERE project_id = ? ORDER BY tag",
                    id_str2
                )
                .fetch_all(db())
                .await
                .map_err(Error::from)
            },
            // Audiences
            async {
                sqlx::query_scalar!(
                    "SELECT audience FROM project_audiences WHERE project_id = ? ORDER BY audience",
                    id_str3
                )
                .fetch_all(db())
                .await
                .map_err(Error::from)
            },
            // Participant files (additional resources): a list of name -> URL pairs.
            async {
                sqlx::query!(
                    "SELECT name, url FROM project_participant_files WHERE project_id = ? ORDER BY name, url",
                    id_str6
                )
                .fetch_all(db())
                .await
                .map(|rows| {
                    rows.into_iter()
                        .map(|row| ParticipantFile {
                            name: row.name,
                            url: row.url,
                        })
                        .collect::<Vec<_>>()
                })
                .map_err(Error::from)
            },
            // Likes: count is cached per-project; per-user liked is fetched live.
            async {
                let count = cached_like_count(project_id).await?;
                let liked = if let Some(ref user) = user_info_clone {
                    user_has_liked(project_id, user.user_id).await?
                } else {
                    false
                };
                Ok::<_, Error>((count, liked))
            },
            // User-specific engagement (saved, joined)
            async {
                if let Some(ref user) = user_info_clone {
                    let (saved, joined) = tokio::try_join!(
                        async {
                            let resp: BookmarkCheckResponse = request(
                                "user.bookmarks.check",
                                BookmarkCheckRequest {
                                    user_id: user.user_id,
                                    project_id: id_str4.clone(),
                                },
                            )
                            .await?;
                            Ok::<_, Error>(resp.has_bookmark)
                        },
                        async {
                            let resp: ProjectJoinCheckResponse = request(
                                "user.joins.check",
                                ProjectJoinCheckRequest {
                                    user_id: user.user_id,
                                    project_id: id_str4.clone(),
                                },
                            )
                            .await?;
                            Ok::<_, Error>(resp.has_joined)
                        },
                    )?;
                    Ok((saved, joined))
                } else {
                    Ok((false, false))
                }
            },
            // Reviews: cached per-project (public list + total public count).
            async {
                let resp = cached_project_reviews(project_id).await?;
                let reviews = enrich_reviews(resp.reviews).await?;
                Ok::<_, Error>((reviews, resp.count))
            },
            // Member count (lazy cached)
            async {
                let cached = get_cached(&ParticipantCount { project_id }).await?;
                cached
                    .parse::<i64>()
                    .map_err(|e| anyhow::anyhow!("parse member count: {e}"))
            },
            // Last edited (lazy cached)
            async {
                let cached = get_cached(&LastEdited {
                    target_id: project_id,
                })
                .await?;
                Ok::<_, Error>(cached)
            },
            // Edit permission check
            async {
                if let Some(ref user) = user_info_clone2 {
                    if user.is_staff || user.is_superuser {
                        Ok::<_, Error>((true, true))
                    } else {
                        let user_str = user.user_id.to_string();
                        let exists = sqlx::query_scalar!(
                            r#"SELECT 1 as "v!: i32" FROM project_page_editors WHERE project_id = ? AND user_id = ?"#,
                            id_str5,
                            user_str
                        )
                        .fetch_optional(db())
                        .await?
                        .is_some();
                        Ok((exists, false))
                    }
                } else {
                    Ok((false, false))
                }
            },
        )?;

        let (saved, joined) = engagement;
        let (reviews, review_count) = reviews_and_count;
        let (like_count, user_has_liked) = likes;
        let (can_edit, is_staff_editor) = edit_permission;

        let additional_fields = if can_edit {
            super::detail_page_edit::load_additional_fields(
                &self.target_id.to_string(),
                DetailPageKind::Project,
            )
            .await
            .unwrap_or_default()
        } else {
            Vec::new()
        };

        // Render editable fields from the row we already loaded — no extra queries.
        let field_values: HashMap<&'static str, String> = HashMap::from([
            ("presenter", row.presenter.clone()),
            ("name", row.name.clone()),
            ("url", row.url.clone()),
            ("goal", row.goal.clone()),
            ("task", row.task.clone()),
            ("description", row.description.clone()),
            (
                "entirely_online",
                (if row.entirely_online { "true" } else { "false" }).to_string(),
            ),
            ("location_name", row.location_name.clone()),
            ("location_description", row.location_description.clone()),
            ("address", row.address.clone()),
            ("city", row.city.clone()),
            ("state", row.state.clone()),
            ("zip", row.zip.clone()),
            ("country", row.country.clone()),
            ("how_to_join", row.how_to_join.clone()),
            ("video_url", row.video_url.clone()),
            (
                "total_expense",
                row.total_expense.map(|v| v.to_string()).unwrap_or_default(),
            ),
            ("time_per_participation", row.time_per_participation.clone()),
            ("audiences", audiences.join(", ")),
            ("participation_schedule", row.participation_schedule.clone()),
            ("mobile_app_url_android", row.mobile_app_url_android.clone()),
            ("mobile_app_url_ios", row.mobile_app_url_ios.clone()),
            ("mobile_app_url", row.mobile_app_url.clone()),
            ("training_materials", row.training_materials.clone()),
            ("classroom_materials", row.classroom_materials.clone()),
            ("facebook_page", row.facebook_page.clone()),
            ("twitter_name", row.twitter_name.clone()),
            ("instagram_page", row.instagram_page.clone()),
            ("tags", tags.join(", ")),
        ]);
        let editable_fields = super::detail_page_edit::render_fields(
            &id_str,
            DetailPageKind::Project,
            can_edit,
            &field_values,
        )
        .await;

        let status: DetailPageStatus = row.status.into();
        let last_updated = if last_edited_str.is_empty() {
            TimeString::try_from(row.created).ok()
        } else {
            TimeString::try_from(last_edited_str).ok()
        };

        let (description_html, description_delta) =
            content_to_html(&row.description).unwrap_or_else(|_| (row.description.clone(), None));
        let (how_to_join_html, how_to_join_delta) =
            content_to_html(&row.how_to_join).unwrap_or_else(|_| (row.how_to_join.clone(), None));

        let meta_description = common::quill::delta_to_excerpt(
            description_delta.as_deref().unwrap_or(&row.description),
            200,
        );

        // Save back normalized Delta so future renders skip conversion
        if description_delta.is_some() || how_to_join_delta.is_some() {
            let project_id = self.target_id;
            tokio::spawn(async move {
                if let Some(delta) = description_delta {
                    let field = crate::project::field::ProjectField {
                        project_id,
                        field: crate::project::field::ProjectFieldValue::Description(delta),
                        source: None,
                    };
                    if let Err(e) = field.store().await {
                        tracing::warn!("Failed to save normalized description delta: {e}");
                    }
                }
                if let Some(delta) = how_to_join_delta {
                    let field = crate::project::field::ProjectField {
                        project_id,
                        field: crate::project::field::ProjectFieldValue::HowToJoin(delta),
                        source: None,
                    };
                    if let Err(e) = field.store().await {
                        tracing::warn!("Failed to save normalized how_to_join delta: {e}");
                    }
                }
            });
        }

        Ok(DetailPageContent {
            kind: DetailPageKind::Project,
            editable_fields,
            slug: self.slug,
            target_id: self.target_id.to_string(),
            name: row.name,
            presenter: row.presenter,
            image: row.image,
            image_credit: row.image_credit,
            url: row.url,
            status,
            like_count,
            user_has_liked,
            saved,
            joined,
            goal: row.goal,
            task: row.task,
            description: description_html,
            meta_description,
            how_to_join: how_to_join_html,
            location_type: row.location_type,
            entirely_online: row.entirely_online,
            location_name: row.location_name,
            location_description: row.location_description,
            address: row.address,
            city: row.city,
            state: row.state,
            zip: row.zip,
            country: row.country,
            has_point: row.has_point,
            total_expense: row
                .total_expense
                .map(|v| {
                    if v == 0.0 {
                        "Free".to_string()
                    } else {
                        format!("${:.2}", v)
                    }
                })
                .unwrap_or_default(),
            time_per_participation: row.time_per_participation,
            audiences,
            participation_schedule: row.participation_schedule,
            mobile_app_url: row.mobile_app_url,
            mobile_app_url_ios: row.mobile_app_url_ios,
            mobile_app_url_android: row.mobile_app_url_android,
            training_materials: row.training_materials,
            classroom_materials: row.classroom_materials,
            participant_files,
            video_url: row.video_url,
            twitter_name: row.twitter_name,
            facebook_page: row.facebook_page,
            instagram_page: row.instagram_page,
            tags,
            last_updated,
            member_count,
            event_type: None,
            instances: Vec::new(),
            user_info,
            reviews,
            review_count,
            can_edit,
            is_staff_editor,
            additional_fields,
        })
    }

    async fn prepare_event(
        self,
        user_info: Option<Arc<BasicInfo>>,
    ) -> Result<DetailPageContent, Error> {
        let id_str = self.target_id.to_string();

        let row = sqlx::query!(
            r#"
            SELECT
                name,
                status as "status: EventStatus",
                presenter,
                description,
                image,
                image_credit,
                url,
                how_to_join,
                location_type,
                entirely_online as "entirely_online: bool",
                location_name,
                location_description,
                address,
                city,
                state,
                zip,
                country,
                event_type as "event_type: EventType",
                point IS NOT NULL as "has_point: bool"
            FROM events
            WHERE event_id = ?
            "#,
            id_str
        )
        .fetch_optional(db())
        .await?
        .ok_or_else(|| anyhow::anyhow!("Event not found: {}", self.target_id))?;

        let id_str2 = id_str.clone();
        let id_str3 = id_str.clone();
        let id_str4 = id_str.clone();
        let event_id = self.target_id;
        let user_info_clone = user_info.clone();
        let user_info_likes = user_info.clone();

        let (tags, instances, likes, edit_permission) = tokio::try_join!(
            // Tags
            async {
                sqlx::query_scalar!(
                    "SELECT tag FROM event_tags WHERE event_id = ? ORDER BY tag",
                    id_str2
                )
                .fetch_all(db())
                .await
                .map_err(Error::from)
            },
            // Instances
            async {
                let rows = sqlx::query!(
                    "SELECT begin, end FROM event_instances WHERE event_id = ? ORDER BY begin",
                    id_str3
                )
                .fetch_all(db())
                .await?;

                Ok::<_, Error>(
                    rows.into_iter()
                        .filter_map(|r| {
                            let begin = TimeString::try_from(r.begin).ok()?;
                            let end = TimeString::try_from(r.end).ok()?;
                            Some(EventInstanceDisplay { begin, end })
                        })
                        .collect::<Vec<_>>(),
                )
            },
            // Likes: count is cached per-project; per-user liked is fetched live.
            async {
                let count = cached_like_count(event_id).await?;
                let liked = if let Some(ref user) = user_info_likes {
                    user_has_liked(event_id, user.user_id).await?
                } else {
                    false
                };
                Ok::<_, Error>((count, liked))
            },
            // Edit permission check
            async {
                if let Some(ref user) = user_info_clone {
                    if user.is_staff || user.is_superuser {
                        Ok::<_, Error>((true, true))
                    } else {
                        let user_str = user.user_id.to_string();
                        let exists = sqlx::query_scalar!(
                            r#"SELECT 1 as "v!: i32" FROM event_page_editors WHERE event_id = ? AND user_id = ?"#,
                            id_str4,
                            user_str
                        )
                        .fetch_optional(db())
                        .await?
                        .is_some();
                        Ok((exists, false))
                    }
                } else {
                    Ok((false, false))
                }
            },
        )?;

        let (like_count, user_has_liked) = likes;
        let (can_edit, is_staff_editor) = edit_permission;

        let additional_fields = if can_edit {
            super::detail_page_edit::load_additional_fields(
                &self.target_id.to_string(),
                DetailPageKind::Event,
            )
            .await
            .unwrap_or_default()
        } else {
            Vec::new()
        };

        // Render editable fields from the row we already loaded — no extra queries.
        let field_values: HashMap<&'static str, String> = HashMap::from([
            ("presenter", row.presenter.clone()),
            ("name", row.name.clone()),
            ("event_type", row.event_type.to_string()),
            ("url", row.url.clone()),
            ("description", row.description.clone()),
            (
                "entirely_online",
                (if row.entirely_online { "true" } else { "false" }).to_string(),
            ),
            ("location_name", row.location_name.clone()),
            ("location_description", row.location_description.clone()),
            ("address", row.address.clone()),
            ("city", row.city.clone()),
            ("state", row.state.clone()),
            ("zip", row.zip.clone()),
            ("country", row.country.clone()),
            ("how_to_join", row.how_to_join.clone()),
            ("tags", tags.join(", ")),
        ]);
        let editable_fields = super::detail_page_edit::render_fields(
            &id_str,
            DetailPageKind::Event,
            can_edit,
            &field_values,
        )
        .await;

        let status: DetailPageStatus = row.status.into();
        let event_type_display = Some(row.event_type.to_string());

        let (description_html, description_delta) =
            content_to_html(&row.description).unwrap_or_else(|_| (row.description.clone(), None));
        let (how_to_join_html, how_to_join_delta) =
            content_to_html(&row.how_to_join).unwrap_or_else(|_| (row.how_to_join.clone(), None));

        let meta_description = common::quill::delta_to_excerpt(
            description_delta.as_deref().unwrap_or(&row.description),
            200,
        );

        // Save back normalized Delta so future renders skip conversion
        if description_delta.is_some() || how_to_join_delta.is_some() {
            let event_id = self.target_id;
            tokio::spawn(async move {
                if let Some(delta) = description_delta {
                    let field = crate::event::field::EventField {
                        event_id,
                        field: crate::event::field::EventFieldValue::Description(delta),
                    };
                    if let Err(e) = field.store().await {
                        tracing::warn!("Failed to save normalized event description delta: {e}");
                    }
                }
                if let Some(delta) = how_to_join_delta {
                    let field = crate::event::field::EventField {
                        event_id,
                        field: crate::event::field::EventFieldValue::HowToJoin(delta),
                    };
                    if let Err(e) = field.store().await {
                        tracing::warn!("Failed to save normalized event how_to_join delta: {e}");
                    }
                }
            });
        }

        Ok(DetailPageContent {
            kind: DetailPageKind::Event,
            editable_fields,
            slug: self.slug,
            target_id: self.target_id.to_string(),
            name: row.name,
            presenter: row.presenter,
            image: row.image,
            image_credit: row.image_credit,
            url: row.url,
            status,
            like_count,
            user_has_liked,
            saved: false,
            joined: false,
            goal: String::new(),
            task: String::new(),
            description: description_html,
            meta_description,
            how_to_join: how_to_join_html,
            location_type: row.location_type,
            entirely_online: row.entirely_online,
            location_name: row.location_name,
            location_description: row.location_description,
            address: row.address,
            city: row.city,
            state: row.state,
            zip: row.zip,
            country: row.country,
            has_point: row.has_point,
            total_expense: String::new(),
            time_per_participation: String::new(),
            audiences: Vec::new(),
            participation_schedule: String::new(),
            mobile_app_url: String::new(),
            mobile_app_url_ios: String::new(),
            mobile_app_url_android: String::new(),
            training_materials: String::new(),
            classroom_materials: String::new(),
            participant_files: Vec::new(),
            video_url: String::new(),
            twitter_name: String::new(),
            facebook_page: String::new(),
            instagram_page: String::new(),
            tags,
            last_updated: None,
            member_count: 0,
            event_type: event_type_display,
            instances,
            user_info,
            reviews: Vec::new(),
            review_count: 0,
            can_edit,
            is_staff_editor,
            additional_fields,
        })
    }
}

// ---------------------------------------------------------------------------
// Review enrichment helper
// ---------------------------------------------------------------------------

async fn enrich_reviews(reviews: Vec<ReviewEntry>) -> Result<Vec<ReviewForDisplay>, Error> {
    let user_ids: Vec<Ulid> = reviews
        .iter()
        .filter_map(|r| Ulid::from_string(&r.user_id).ok())
        .collect();

    let user_info_map = if !user_ids.is_empty() {
        let response: UserDisplayInfoBatchResponse = request(
            "user.info.display_info_batch",
            UserDisplayInfoBatchRequest { user_ids },
        )
        .await?;

        response
            .users
            .into_iter()
            .map(|u| (u.user_id.to_string(), (u.handle, u.image)))
            .collect::<HashMap<_, _>>()
    } else {
        HashMap::new()
    };

    Ok(reviews
        .into_iter()
        .map(|review| {
            let (user_handle, user_image) = user_info_map
                .get(&review.user_id)
                .cloned()
                .unwrap_or_else(|| (format!("({})", review.user_id), String::new()));

            ReviewForDisplay {
                user_id: review.user_id,
                user_handle,
                user_image,
                text: review.text,
                when: TimeString::try_from(review.when).unwrap_or_else(|_| TimeString::now()),
            }
        })
        .collect())
}

// ---------------------------------------------------------------------------
// HTMX endpoint handlers
// ---------------------------------------------------------------------------

/// Toggle like on a project/event
#[handler]
pub async fn toggle_like(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(|| {
        poem::Error::from_string(
            "Authentication required",
            poem::http::StatusCode::UNAUTHORIZED,
        )
    })?;
    let user_id = user.user_id.to_string();

    // Determine the current like state, then flip it. The log-service is the
    // single source of truth for likes (orrery-synced history + new hydra likes).
    let current: ProjectLikesResponse = request(
        "log.project_likes",
        ProjectLikesRequest {
            project_id: id.clone(),
            user_id: Some(user_id.clone()),
        },
    )
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let updated: ProjectLikesResponse = request(
        "log.rating_set",
        RatingSetRequest {
            project_id: id.clone(),
            user_id,
            liked: !current.liked,
        },
    )
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    #[derive(TemplateOnce)]
    #[template(path = "pages/detail_page_like_button.stpl.html")]
    struct LikeButton {
        like_count: i64,
        user_has_liked: bool,
        target_id: String,
    }

    let html = LikeButton {
        like_count: updated.count,
        user_has_liked: updated.liked,
        target_id: id,
    }
    .render_once()
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

/// Save/bookmark a project
#[handler]
pub async fn save_project(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(|| {
        poem::Error::from_string(
            "Authentication required",
            poem::http::StatusCode::UNAUTHORIZED,
        )
    })?;

    let _response: BookmarkCreateResponse = request(
        "user.bookmarks.create",
        BookmarkCreateRequest {
            user_id: user.user_id,
            project_id: id.clone(),
        },
    )
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    #[derive(TemplateOnce)]
    #[template(path = "pages/detail_page_save_button.stpl.html")]
    struct SaveButton {
        saved: bool,
        target_id: String,
    }

    let html = SaveButton {
        saved: true,
        target_id: id,
    }
    .render_once()
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

/// Share modal
#[handler]
pub async fn share_modal(Path(id): Path<String>) -> poem::Result<poem::Response> {
    let id_str = id.clone();

    // Look up the project/event name, slug, kind, and uploaded image
    let row = sqlx::query!(
        r#"
        SELECT
            e.slug,
            e.kind,
            COALESCE(p.name, ev.name) as "name!: String",
            COALESCE(p.image, ev.image) as "image!: String"
        FROM entities e
        LEFT JOIN projects p ON e.target = p.project_id AND e.kind = 'Project'
        LEFT JOIN events ev ON e.target = ev.event_id AND e.kind = 'Event'
        WHERE e.target = ?
        "#,
        id_str
    )
    .fetch_optional(db())
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?
    .ok_or_else(|| poem::Error::from_string("Not found", poem::http::StatusCode::NOT_FOUND))?;

    // Build the canonical, absolute page URL for sharing.
    let path_prefix = match row.kind.as_str() {
        "Event" => "/events/",
        _ => "/projects/",
    };
    let share_url = format!("https://{}{}{}", DOMAIN, path_prefix, row.slug);
    let url_enc = urlencoding::encode(&share_url);
    let name_enc = urlencoding::encode(&row.name);

    #[derive(TemplateOnce)]
    #[template(path = "pages/detail_page_share_modal.stpl.html")]
    struct ShareModal {
        /// Entity ULID, used by share controls to report clicks for analytics.
        target_id: String,
        name: String,
        share_url: String,
        /// Absolute URL of the uploaded image, shared to Instagram. Empty when none.
        image: String,
        facebook_url: String,
        twitter_url: String,
        linkedin_url: String,
    }

    let html = ShareModal {
        facebook_url: format!("https://www.facebook.com/sharer/sharer.php?u={url_enc}"),
        twitter_url: format!("https://twitter.com/intent/tweet?url={url_enc}&text={name_enc}"),
        linkedin_url: format!("https://www.linkedin.com/sharing/share-offsite/?url={url_enc}"),
        target_id: id,
        name: row.name,
        image: row.image,
        share_url,
    }
    .render_once()
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

/// Record a social-share button click (fire-and-forget analytics).
///
/// A share is an action, not an external-content clickthrough, so it is logged
/// to the dedicated `share_events` table via [`log_share`] rather than going
/// through the clickthrough pipeline. Called by a `navigator.sendBeacon` from
/// the share controls, so the response body is irrelevant.
#[handler]
pub async fn record_share(
    Authenticated(user_info): Authenticated,
    device: Device,
    interaction: Interaction,
    Path((id, platform)): Path<(String, String)>,
) -> poem::Result<poem::Response> {
    use poem::http::StatusCode;

    let (Ok(target_id), Ok(platform)) = (id.parse::<Ulid>(), platform.parse::<SharePlatform>())
    else {
        return Ok(StatusCode::BAD_REQUEST.into_response());
    };

    if let Err(e) = log_share(
        None,
        target_id,
        platform,
        user_info.as_ref().map(|u| u.user_id),
        String::from(device),
        String::from(interaction),
    )
    .await
    {
        tracing::warn!("Failed to log share event: {e}");
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Map modal
#[handler]
pub async fn map_modal(Path(id): Path<String>) -> poem::Result<poem::Response> {
    // Try projects first, then events
    let point = sqlx::query_scalar!(
        r#"SELECT AsGeoJSON(point) as "point: String" FROM projects WHERE project_id = ?"#,
        id
    )
    .fetch_optional(db())
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?
    .flatten();

    let point = if point.is_none() {
        sqlx::query_scalar!(
            r#"SELECT AsGeoJSON(point) as "point: String" FROM events WHERE event_id = ?"#,
            id
        )
        .fetch_optional(db())
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?
        .flatten()
    } else {
        point
    };

    let geojson = point.unwrap_or_default();

    #[derive(TemplateOnce)]
    #[template(path = "pages/detail_page_map_modal.stpl.html")]
    struct MapModal {
        geojson: String,
    }

    let html = MapModal { geojson }.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

/// Submit a review
#[handler]
pub async fn submit_review(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
    poem::web::Form(form): poem::web::Form<ReviewForm>,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(|| {
        poem::Error::from_string(
            "Authentication required",
            poem::http::StatusCode::UNAUTHORIZED,
        )
    })?;
    // Store the review in the log-service (single source of truth for reviews,
    // unifying orrery-synced history with new hydra reviews).
    let stored: ReviewEntry = request(
        "log.review_set",
        ReviewSetRequest {
            project_id: id.clone(),
            user_id: user.user_id.to_string(),
            text: form.text.clone(),
            private: form.private,
        },
    )
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    // A private review is hidden from the public list, so don't echo a card back.
    if form.private {
        return Ok(Html(String::new()).into_response());
    }

    // Get user display info
    let response: UserDisplayInfoBatchResponse = request(
        "user.info.display_info_batch",
        UserDisplayInfoBatchRequest {
            user_ids: vec![user.user_id],
        },
    )
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let (handle, image) = response
        .users
        .into_iter()
        .next()
        .map(|u| (u.handle, u.image))
        .unwrap_or_else(|| (format!("({})", user.user_id), String::new()));

    #[derive(TemplateOnce)]
    #[template(path = "pages/detail_page_review.stpl.html")]
    struct ReviewCard {
        review: ReviewForDisplay,
        oob: bool,
    }

    let html = ReviewCard {
        review: ReviewForDisplay {
            user_id: user.user_id.to_string(),
            user_handle: handle,
            user_image: image,
            text: stored.text,
            when: TimeString::try_from(stored.when).unwrap_or_else(|_| TimeString::now()),
        },
        oob: true,
    }
    .render_once()
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

#[derive(serde::Deserialize)]
pub struct ReviewForm {
    pub text: String,
    /// Checkbox; absent when unchecked, so default to public.
    #[serde(default)]
    pub private: bool,
}

/// Lists modal - show user's lists to add entity to
#[handler]
pub async fn lists_modal(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let user = user_info.ok_or_else(|| {
        poem::Error::from_string(
            "Authentication required",
            poem::http::StatusCode::UNAUTHORIZED,
        )
    })?;

    let lists = crate::entity_list::EntityList::get_user_lists(user.user_id)
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    #[derive(TemplateOnce)]
    #[template(path = "pages/detail_page_lists_modal.stpl.html")]
    struct ListsModal {
        lists: Vec<crate::entity_list::EntityList>,
        target_id: String,
    }

    let html = ListsModal {
        lists,
        target_id: id,
    }
    .render_once()
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

/// Add entity to list
#[handler]
pub async fn add_to_list(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
    poem::web::Form(form): poem::web::Form<AddToListForm>,
) -> poem::Result<poem::Response> {
    let _user = user_info.ok_or_else(|| {
        poem::Error::from_string(
            "Authentication required",
            poem::http::StatusCode::UNAUTHORIZED,
        )
    })?;

    // Look up entity slug
    let slug = sqlx::query_scalar!("SELECT slug FROM entities WHERE target = ?", id)
        .fetch_optional(db())
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?
        .ok_or_else(|| {
            poem::Error::from_string("Entity not found", poem::http::StatusCode::NOT_FOUND)
        })?;

    let list_id = Ulid::from_string(&form.list_id).map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::BAD_REQUEST)
    })?;

    let entry = crate::entity_list::EntityListEntry {
        entry_id: Ulid::new(),
        list_id,
        priority: 0,
        added_at: TimeString::now().to_string(),
        entity_slug: slug,
        note: String::new(),
        link: String::new(),
    };
    entry.store().await.map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html("<div>Added to list!</div>").into_response())
}

#[derive(serde::Deserialize)]
pub struct AddToListForm {
    pub list_id: String,
}

/// Record "I already joined" participation
#[handler]
pub async fn record_join(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    use common::message::log::ProjectParticipationRecord;

    let user = user_info.ok_or_else(|| {
        poem::Error::from_string(
            "Authentication required",
            poem::http::StatusCode::UNAUTHORIZED,
        )
    })?;

    send(
        "log.project_participation_record",
        ProjectParticipationRecord {
            user_id: user.user_id,
            project_id: id,
            origin: Some(0),
            when: None,
            duration: None,
            where_point: None,
            magnitude: None,
            authenticated: Some(true),
            extra: None,
            fidelity: None,
            task_name: None,
        },
    )
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html("<div class=\"text-sm text-green-700\">Participation recorded!</div>").into_response())
}

/// Staff-only edit log (lazy loaded via HTMX)
#[handler]
pub async fn edit_log(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    use common::message::log::{EditLogRequest, EditLogResponse};

    let user = user_info.ok_or_else(|| {
        poem::Error::from_string(
            "Authentication required",
            poem::http::StatusCode::UNAUTHORIZED,
        )
    })?;

    if !user.is_staff && !user.is_superuser {
        return Err(poem::Error::from_string(
            "Forbidden",
            poem::http::StatusCode::FORBIDDEN,
        ));
    }

    const LOG_LIMIT: i64 = 5;

    let resp: EditLogResponse = request(
        "log.edit_log",
        EditLogRequest {
            target_id: id.clone(),
            limit: LOG_LIMIT,
        },
    )
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let edit_count = resp.entries.len() as i64;

    // Resolve user IDs to display names
    let user_ids: Vec<Ulid> = resp
        .entries
        .iter()
        .filter_map(|e| Ulid::from_string(&e.user_id).ok())
        .collect();

    let user_map = if !user_ids.is_empty() {
        let user_resp: UserDisplayInfoBatchResponse = request(
            "user.info.display_info_batch",
            UserDisplayInfoBatchRequest { user_ids },
        )
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

        user_resp
            .users
            .into_iter()
            .map(|u| (u.user_id.to_string(), u.handle))
            .collect::<HashMap<_, _>>()
    } else {
        HashMap::new()
    };

    let mut entries: Vec<EditLogEntryDisplay> = resp
        .entries
        .into_iter()
        .map(|e| {
            let user_handle = user_map
                .get(&e.user_id)
                .cloned()
                .unwrap_or_else(|| e.user_id.clone());
            EditLogEntryDisplay {
                when: TimeString::try_from(e.when).ok(),
                user_handle,
                field: e.field,
                is_creation: false,
            }
        })
        .collect();

    // If the log has fewer entries than the limit, append the creation date
    if edit_count < LOG_LIMIT {
        let row = sqlx::query!(
            r#"SELECT COALESCE(
                (SELECT created FROM projects WHERE project_id = ?),
                (SELECT created FROM events WHERE event_id = ?)
            ) as "created: String",
            COALESCE(
                (SELECT page_creator_id FROM projects WHERE project_id = ?),
                (SELECT page_creator_id FROM events WHERE event_id = ?)
            ) as "creator_id: String""#,
            id,
            id,
            id,
            id
        )
        .fetch_optional(db())
        .await
        .map_err(|e| {
            poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;

        if let Some(row) = row {
            if let Some(created_str) = row.created {
                let when_display = TimeString::try_from(created_str).ok();

                // Look up creator name if we have a creator ID
                let creator_handle = if let Some(ref creator_id) = row.creator_id {
                    if let Ok(creator_ulid) = Ulid::from_string(creator_id) {
                        let resp: Result<UserDisplayInfoBatchResponse, _> = request(
                            "user.info.display_info_batch",
                            UserDisplayInfoBatchRequest {
                                user_ids: vec![creator_ulid],
                            },
                        )
                        .await;
                        resp.ok()
                            .and_then(|r| r.users.into_iter().next())
                            .map(|u| u.handle)
                            .unwrap_or_default()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };

                entries.push(EditLogEntryDisplay {
                    when: when_display,
                    user_handle: creator_handle,
                    field: "Created".to_string(),
                    is_creation: true,
                });
            }
        }
    }

    #[derive(TemplateOnce)]
    #[template(path = "pages/detail_page_edit_log.stpl.html")]
    struct EditLog {
        entries: Vec<EditLogEntryDisplay>,
    }

    let html = EditLog { entries }.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

struct EditLogEntryDisplay {
    when: Option<TimeString>,
    user_handle: String,
    field: String,
    is_creation: bool,
}
