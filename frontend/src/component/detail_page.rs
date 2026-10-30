use std::sync::Arc;
use std::time::Duration;

use anyhow::Error;
use common::{
    linked_platform::LinkedPlatform,
    message::{
        dashboard::{UserProfile, UserProfileUpdate},
        log::{
            LastChangedRequest, LastChangedResponse, ProjectLikesRequest, ProjectLikesResponse,
            ProjectMemberCountRequest, ProjectMemberCountResponse, ProjectReviewsRequest,
            ProjectReviewsResponse, RatingSetRequest, ReviewEntry, ReviewSetRequest, SharePlatform,
        },
        user::{
            BasicInfo, BookmarkCheckRequest, BookmarkCheckResponse, BookmarkCreateRequest,
            BookmarkCreateResponse, ProjectJoinCheckRequest, ProjectJoinCheckResponse,
            ProjectJoinRequest, ProjectJoinResponse, UserDisplayInfoBatchRequest,
            UserDisplayInfoBatchResponse,
        },
    },
    service::{log_share, request, send},
    shared::db,
    timestring::TimeString,
};
use poem::{
    IntoResponse, handler,
    session::Session,
    web::{Form, Html, Path, Query, RealIp},
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
use crate::project::linked_platform::{NamedAccount, PlatformCopy, name_account};
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

/// Whether the viewer may edit the page, and whether as staff.
async fn edit_permission(
    user: Option<&BasicInfo>,
    page: crate::edit_access::Page,
) -> Result<(bool, bool), Error> {
    let Some(user) = user else {
        return Ok((false, false));
    };

    let editor = crate::edit_access::Editor::from(user);
    Ok((
        crate::edit_access::may_edit(page, editor).await?,
        editor.is_staff,
    ))
}

impl DetailPageKind {
    /// The page of this kind that `target` names, as edit access knows it.
    pub fn page(self, target: Ulid) -> crate::edit_access::Page {
        match self {
            DetailPageKind::Project => crate::edit_access::Page::Project(target),
            DetailPageKind::Event => crate::edit_access::Page::Event(target),
        }
    }

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

/// When a page last changed, cached on each frontend instance.
#[derive(Clone)]
struct LastChanged {
    target_id: Ulid,
}

impl LazyCached for LastChanged {
    fn cache_key(&self) -> String {
        format!("last_changed:{}", self.target_id)
    }

    fn max_age(&self) -> Duration {
        Duration::from_secs(3600)
    }

    async fn fetch_fresh(&self) -> Result<String, Error> {
        let resp: LastChangedResponse = request(
            "log.last_changed",
            LastChangedRequest {
                targets: vec![self.target_id],
            },
        )
        .await?;
        Ok(resp
            .changes
            .into_iter()
            .next()
            .map(|change| change.when.to_string())
            .unwrap_or_default())
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
    /// Whether an account signed up for from this page would be protected
    /// (`auth::signup_will_be_protected`), which only the session knows.
    pub signup_protected: bool,
}

// ---------------------------------------------------------------------------
// Prepared struct (template)
// ---------------------------------------------------------------------------

/// One organizer contact, for display on the event page (editors only).
#[derive(Debug, Clone)]
pub struct ContactDisplay {
    pub contact_id: String,
    pub name: String,
    pub affiliation: String,
    pub email: String,
    pub phone: String,
}

/// One non-staff page manager (editor), for display on the event page (editors only).
#[derive(Debug, Clone)]
pub struct ManagerDisplay {
    pub user_id: String,
    pub email: String,
}

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
    /// Whether the project credits participation to the participant's
    /// SciStarter account.
    pub is_affiliate: bool,
    pub howto_video: String,
    /// What the "How to Get Started" box says ahead of `how_to_join`.
    pub guidance: GetStartedGuidance,

    // Location
    pub location_type: String,
    pub entirely_online: bool,
    pub location_name: String,
    /// Name of the geographic area a NEAR-located project covers ("Middleborough,
    /// MA"). Projects imported without an address carry their only human-readable
    /// location here, so the Where card must show it. Always empty for events.
    pub regions_description: String,
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
    pub ticketing_begins: Option<TimeString>,
    /// The umbrella event this one is part of (name + path), empty if none.
    pub part_of_name: String,
    pub part_of_path: String,
    /// The event's region as a GeoJSON string, for the inline map.
    pub region_geojson: Option<String>,
    pub contacts: Vec<ContactDisplay>,
    /// Non-staff page managers (editors), shown to editors only.
    pub managers: Vec<ManagerDisplay>,

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

    /// What an event's organizer said about promoting it on Facebook, shown
    /// to editors only. Always unanswered for a project.
    pub facebook_event: crate::event::facebook::FacebookEvent,

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

/// What the "How to Get Started" box says ahead of the project's own
/// instructions, in the order the cases are tried.
#[derive(Clone)]
pub enum GetStartedGuidance {
    /// The project's own replacement for everything below, in the variant for
    /// a signed-in or a signed-out visitor. Staff-written HTML.
    Custom(String),
    /// Connect an account on the platform that credits this project.
    LinkedPlatform(PlatformAccountForm),
    /// An affiliate that credits participation recorded under the
    /// participant's SciStarter email address.
    Affiliate,
    Nothing,
}

impl GetStartedGuidance {
    /// The guidance for a project page, by the precedence orrery's project
    /// page gives it.
    fn for_project(
        signed_in: bool,
        prefix_logged_in: String,
        prefix_logged_out: String,
        is_affiliate: bool,
        platform_account: Option<PlatformAccountForm>,
    ) -> Self {
        let custom = if signed_in {
            prefix_logged_in
        } else {
            prefix_logged_out
        };

        if !custom.is_empty() {
            GetStartedGuidance::Custom(custom)
        } else if let Some(form) = platform_account {
            GetStartedGuidance::LinkedPlatform(form)
        } else if is_affiliate {
            GetStartedGuidance::Affiliate
        } else {
            GetStartedGuidance::Nothing
        }
    }

    pub fn is_nothing(&self) -> bool {
        matches!(self, GetStartedGuidance::Nothing)
    }
}

/// The form a visitor names their username on a project's platform with, or
/// for a visitor who is not signed in, the way to sign in first.
#[derive(TemplateOnce, Clone)]
#[template(path = "pages/detail_page_platform_account.stpl.html")]
pub struct PlatformAccountForm {
    pub target_id: String,
    pub platform: LinkedPlatform,
    pub visitor: PlatformAccountVisitor,
    pub outcome: PlatformAccountOutcome,
    /// Whether the page lists the project's mobile apps, further down.
    pub has_app_links: bool,
}

#[derive(Clone)]
pub enum PlatformAccountVisitor {
    /// Signed in, with the username they have named on the platform, "" if
    /// none.
    SignedIn(String),
    /// Not signed in, and offered the way to.
    SignedOut(InlineAuthForm),
}

/// A sign-in / create-account form that sits in a project page, rather than
/// a link to /login. Either way the visitor comes back signed in to the same
/// page, having joined the project.
#[derive(TemplateOnce, Clone)]
#[template(path = "pages/detail_page_inline_auth.stpl.html")]
pub struct InlineAuthForm {
    pub target_id: String,
    /// The project's page, where a signup finished from an emailed link
    /// lands, and the full login page sends a passkey holder back to.
    pub page_path: String,
    pub mode: InlineAuthMode,
    /// The email address typed so far.
    pub email: String,
    /// Whether to offer the newsletter: a protected account never gets it.
    pub offer_newsletter: bool,
    pub problem: Option<InlineAuthProblem>,
    /// Set once a signup has been deferred to the link emailed here.
    pub pending_email: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InlineAuthMode {
    SignUp,
    SignIn,
}

/// Why the inline form is being shown again.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InlineAuthProblem {
    WrongPassword,
    EmailTaken,
    PasswordTooShort,
    RateLimited,
    Unavailable,
}

impl InlineAuthProblem {
    pub fn message(self) -> &'static str {
        match self {
            InlineAuthProblem::WrongPassword => "Incorrect email or password.",
            InlineAuthProblem::EmailTaken => {
                "That email address already belongs to a SciStarter account. Sign in instead."
            }
            InlineAuthProblem::PasswordTooShort => {
                "Please choose a password of at least 8 characters."
            }
            InlineAuthProblem::RateLimited => {
                "Too many signup attempts from your network right now. Please try again in an hour."
            }
            InlineAuthProblem::Unavailable => {
                "Something went wrong on our end. Please wait a little while and then try again."
            }
        }
    }
}

/// The shortest password the inline signup accepts, as orrery's does.
const MINIMUM_PASSWORD_LENGTH: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PlatformAccountOutcome {
    Unsubmitted,
    Saved,
    Blank,
    /// The platform has no account with the username given.
    Unknown,
}

impl PlatformAccountOutcome {
    /// Whether the username given was refused, leaving the visitor to try
    /// again.
    pub fn refused(self) -> bool {
        matches!(
            self,
            PlatformAccountOutcome::Blank | PlatformAccountOutcome::Unknown
        )
    }
}

/// The path a project's page is served at.
pub fn project_page_path(slug: &str) -> String {
    format!("{}{slug}", crate::entity::EntityKind::Project.path_prefix())
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
                regions_description,
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
                created,
                is_affiliate as "is_affiliate: bool",
                is_inaturalist as "is_inaturalist: bool",
                prefix_how_to_logged_in,
                prefix_how_to_logged_out,
                howto_video
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
        let id_str6 = id_str.clone();
        let user_info_clone = user_info.clone();
        let user_info_clone2 = user_info.clone();

        let linked_platform = crate::project::linked_platform::for_project(
            project_id,
            row.is_affiliate,
            row.is_inaturalist,
        )
        .await?;

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
            platform_username,
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
            // Last changed (lazy cached)
            async {
                let cached = get_cached(&LastChanged {
                    target_id: project_id,
                })
                .await?;
                Ok::<_, Error>(cached)
            },
            // Edit permission check
            edit_permission(
                user_info_clone2.as_deref(),
                DetailPageKind::Project.page(project_id)
            ),
            // The visitor's username on the platform that credits this project
            async {
                match (linked_platform, &user_info_clone) {
                    (Some(platform), Some(user)) => {
                        let profile: UserProfile =
                            request("user.profile.get", user.user_id.to_string()).await?;
                        Ok::<_, Error>(Some(profile.platform_username(platform).to_string()))
                    }
                    _ => Ok(None),
                }
            },
        )?;

        let (saved, joined) = engagement;
        let (reviews, review_count) = reviews_and_count;
        let (like_count, user_has_liked) = likes;
        let (can_edit, is_staff_editor) = edit_permission;

        // Hide internal `_`-prefixed tags from the public view; editors still see
        // (and can manage) them in the editable field.
        let tags = if can_edit {
            tags
        } else {
            tags.into_iter()
                .filter(|t| !crate::project::is_hidden_tag(t))
                .collect::<Vec<_>>()
        };

        // Organizer contacts (rendered only for editors — see the template).
        let contacts = sqlx::query!(
            r#"SELECT contact_id AS "contact_id!", name, affiliation, email AS "email!", phone
               FROM project_contacts WHERE project_id = ? ORDER BY role"#,
            id_str
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|c| ContactDisplay {
            contact_id: c.contact_id,
            name: c.name.unwrap_or_default(),
            affiliation: c.affiliation.unwrap_or_default(),
            email: c.email,
            phone: c.phone.unwrap_or_default(),
        })
        .collect::<Vec<_>>();

        // Non-staff page managers, resolved to emails (editors only).
        let managers = if can_edit {
            let user_ids: Vec<Ulid> = sqlx::query_scalar!(
                r#"SELECT user_id FROM project_page_editors WHERE project_id = ? ORDER BY user_id"#,
                id_str
            )
            .fetch_all(db())
            .await?
            .into_iter()
            .filter_map(|s| s.parse::<Ulid>().ok())
            .collect();

            let resolved: HashMap<Ulid, String> =
                crate::admin::autocomplete::resolve_user_names(user_ids.clone())
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .collect();

            user_ids
                .into_iter()
                .map(|uid| ManagerDisplay {
                    user_id: uid.to_string(),
                    email: resolved
                        .get(&uid)
                        .cloned()
                        .unwrap_or_else(|| uid.to_string()),
                })
                .collect()
        } else {
            Vec::new()
        };

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
            ("regions_description", row.regions_description.clone()),
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

        let guidance = GetStartedGuidance::for_project(
            user_info.is_some(),
            row.prefix_how_to_logged_in,
            row.prefix_how_to_logged_out,
            row.is_affiliate,
            linked_platform.map(|platform| PlatformAccountForm {
                target_id: self.target_id.to_string(),
                platform,
                visitor: match platform_username {
                    Some(username) => PlatformAccountVisitor::SignedIn(username),
                    None => PlatformAccountVisitor::SignedOut(InlineAuthForm {
                        target_id: self.target_id.to_string(),
                        page_path: project_page_path(&self.slug),
                        mode: InlineAuthMode::SignUp,
                        email: String::new(),
                        offer_newsletter: !self.signup_protected,
                        problem: None,
                        pending_email: None,
                    }),
                },
                outcome: PlatformAccountOutcome::Unsubmitted,
                has_app_links: !row.mobile_app_url.is_empty()
                    || !row.mobile_app_url_ios.is_empty()
                    || !row.mobile_app_url_android.is_empty(),
            }),
        );

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
            is_affiliate: row.is_affiliate,
            howto_video: row.howto_video,
            guidance,
            location_type: row.location_type,
            entirely_online: row.entirely_online,
            location_name: row.location_name,
            regions_description: row.regions_description,
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
            ticketing_begins: None,
            part_of_name: String::new(),
            part_of_path: String::new(),
            region_geojson: None,
            contacts,
            managers,
            user_info,
            reviews,
            review_count,
            can_edit,
            is_staff_editor,
            additional_fields,
            facebook_event: crate::event::facebook::FacebookEvent::Unanswered,
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
                ticketing_begins,
                part_of,
                AsGeoJSON(regions) as "region_geojson: Option<String>",
                point IS NOT NULL as "has_point: bool",
                facebook_event as "facebook_event: bool",
                facebook_event_url,
                facebook_event_requested as "facebook_event_requested: bool"
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
            edit_permission(
                user_info_clone.as_deref(),
                DetailPageKind::Event.page(event_id)
            ),
        )?;

        let (like_count, user_has_liked) = likes;
        let (can_edit, is_staff_editor) = edit_permission;

        // Hide internal `_`-prefixed tags from the public view; editors still see
        // (and can manage) them in the editable field.
        let tags = if can_edit {
            tags
        } else {
            tags.into_iter()
                .filter(|t| !crate::project::is_hidden_tag(t))
                .collect::<Vec<_>>()
        };

        // The umbrella event this one is part of (only linked if published).
        let (part_of_name, part_of_path) = if let Some(parent_id) = row.part_of.clone() {
            match sqlx::query!(
                r#"SELECT e.name AS "name!", ent.slug AS "slug!"
                   FROM events e
                   JOIN entities ent ON ent.target = e.event_id AND ent.kind = 'Event'
                   WHERE ent.publish = 1 AND e.event_id = ?"#,
                parent_id
            )
            .fetch_optional(db())
            .await?
            {
                Some(r) => (r.name, format!("/events/{}", r.slug)),
                None => (String::new(), String::new()),
            }
        } else {
            (String::new(), String::new())
        };

        // Organizer contacts (rendered only for editors — see the template).
        let contacts = sqlx::query!(
            r#"SELECT contact_id AS "contact_id!", name, affiliation, email AS "email!", phone
               FROM event_contacts WHERE event_id = ? ORDER BY role"#,
            id_str
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|c| ContactDisplay {
            contact_id: c.contact_id,
            name: c.name.unwrap_or_default(),
            affiliation: c.affiliation.unwrap_or_default(),
            email: c.email,
            phone: c.phone.unwrap_or_default(),
        })
        .collect::<Vec<_>>();

        // Non-staff page managers, resolved to emails (editors only).
        let managers = if can_edit {
            let user_ids: Vec<Ulid> = sqlx::query_scalar!(
                r#"SELECT user_id FROM event_page_editors WHERE event_id = ? ORDER BY user_id"#,
                id_str
            )
            .fetch_all(db())
            .await?
            .into_iter()
            .filter_map(|s| s.parse::<Ulid>().ok())
            .collect();

            let resolved: HashMap<Ulid, String> =
                crate::admin::autocomplete::resolve_user_names(user_ids.clone())
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .collect();

            user_ids
                .into_iter()
                .map(|uid| ManagerDisplay {
                    user_id: uid.to_string(),
                    email: resolved
                        .get(&uid)
                        .cloned()
                        .unwrap_or_else(|| uid.to_string()),
                })
                .collect()
        } else {
            Vec::new()
        };

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
            is_affiliate: false,
            howto_video: String::new(),
            guidance: GetStartedGuidance::Nothing,
            location_type: row.location_type,
            entirely_online: row.entirely_online,
            location_name: row.location_name,
            regions_description: String::new(),
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
            ticketing_begins: row
                .ticketing_begins
                .and_then(|s| TimeString::try_from(s).ok()),
            part_of_name,
            part_of_path,
            region_geojson: row.region_geojson.flatten(),
            contacts,
            managers,
            user_info,
            reviews: Vec::new(),
            review_count: 0,
            can_edit,
            is_staff_editor,
            additional_fields,
            facebook_event: crate::event::facebook::FacebookEvent::from_columns(
                row.facebook_event,
                row.facebook_event_url,
                row.facebook_event_requested,
            ),
        })
    }
}

// ---------------------------------------------------------------------------
// Review enrichment helper
// ---------------------------------------------------------------------------

/// Attach author display identity to reviews, dropping those written by a
/// private user. Naming the author as "Private" would not be enough: a
/// review is free text that routinely identifies whoever wrote it, so the
/// whole entry stays out of the public list — the same rule orrery's
/// review query applies.
pub(crate) async fn enrich_reviews(
    reviews: Vec<ReviewEntry>,
) -> Result<Vec<ReviewForDisplay>, Error> {
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
            .map(|u| (u.user_id.to_string(), u))
            .collect::<HashMap<_, _>>()
    } else {
        HashMap::new()
    };

    Ok(reviews
        .into_iter()
        .filter_map(|review| {
            let (user_handle, user_image) = match user_info_map.get(&review.user_id) {
                Some(info) if info.private => return None,
                Some(info) => (info.handle.clone(), info.image.clone()),
                // An author the user service doesn't know (a deleted
                // account, or an id that never was one).
                None => (format!("({})", review.user_id), String::new()),
            };

            Some(ReviewForDisplay {
                user_id: review.user_id,
                user_handle,
                user_image,
                text: review.text,
                when: TimeString::try_from(review.when).unwrap_or_else(|_| TimeString::now()),
            })
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

/// Compose-a-message modal. Lets a logged-in visitor send a message to this
/// project/event; the entity's inbox (a personal forum keyed by the entity
/// ULID) receives it, and its managers read/reply. Mirrors the share modal's
/// fragment+dialog pattern; the form posts to the unified
/// `/dashboard/messages/send`.
#[handler]
pub async fn message_modal(
    Authenticated(user_info): Authenticated,
    Path(id): Path<String>,
) -> poem::Result<poem::Response> {
    use poem::http::StatusCode;

    // Messaging requires a signed-in sender.
    if user_info.is_none() {
        return Err(poem::Error::from_string(
            "Please log in to send a message.",
            StatusCode::UNAUTHORIZED,
        ));
    }

    // Confirm the id is a real project/event and get its name for the title.
    let row = sqlx::query!(
        r#"
        SELECT COALESCE(p.name, ev.name) as "name!: String"
        FROM entities e
        LEFT JOIN projects p ON e.target = p.project_id AND e.kind = 'Project'
        LEFT JOIN events ev ON e.target = ev.event_id AND e.kind = 'Event'
        WHERE e.target = ?
        "#,
        id
    )
    .fetch_optional(db())
    .await
    .map_err(|e| poem::Error::from_string(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR))?
    .ok_or_else(|| poem::Error::from_string("Not found", StatusCode::NOT_FOUND))?;

    #[derive(TemplateOnce)]
    #[template(path = "pages/detail_page_message_modal.stpl.html")]
    struct MessageModal {
        target_id: String,
        name: String,
    }

    let html = MessageModal {
        target_id: id,
        name: row.name,
    }
    .render_once()
    .map_err(|e| poem::Error::from_string(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR))?;

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
        // Adding a project to a list is the act of putting it there; drafting
        // is a curation feature, reached from the list's own editor.
        published: true,
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

#[derive(serde::Deserialize)]
pub struct PlatformAccountSubmission {
    username: String,
}

/// Name the visitor's username on the platform that credits this project.
///
/// Which platform is the project's to say, not the form's, so a submission
/// can only set the username the page asked for. Saving through
/// `user.profile.save` stamps the connection time, the floor for importing
/// that platform's activity.
///
/// The name is read, and a Zooniverse login looked up, by
/// [`name_account`].
#[handler]
pub async fn connect_platform_account(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
    Form(submission): Form<PlatformAccountSubmission>,
) -> poem::Result<poem::Response> {
    let internal = |e: Error| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    };

    let user = user_info.ok_or_else(|| {
        poem::Error::from_string(
            "Authentication required",
            poem::http::StatusCode::UNAUTHORIZED,
        )
    })?;

    let project_id: Ulid = id
        .parse()
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::NOT_FOUND))?;
    let id_str = project_id.to_string();

    let project = sqlx::query!(
        r#"SELECT is_affiliate as "is_affiliate: bool", is_inaturalist as "is_inaturalist: bool",
                  mobile_app_url, mobile_app_url_android, mobile_app_url_ios
           FROM projects WHERE project_id = ?"#,
        id_str
    )
    .fetch_optional(db())
    .await
    .map_err(|e| internal(e.into()))?
    .ok_or_else(|| poem::Error::from_status(poem::http::StatusCode::NOT_FOUND))?;

    let platform = crate::project::linked_platform::for_project(
        project_id,
        project.is_affiliate,
        project.is_inaturalist,
    )
    .await
    .map_err(internal)?
    .ok_or_else(|| poem::Error::from_status(poem::http::StatusCode::NOT_FOUND))?;

    let named = name_account(platform, &submission.username).await;

    let (outcome, username) = match &named {
        NamedAccount::Blank => (PlatformAccountOutcome::Blank, String::new()),
        NamedAccount::Unknown(username) => (PlatformAccountOutcome::Unknown, username.clone()),
        NamedAccount::Named { username, .. } => (PlatformAccountOutcome::Saved, username.clone()),
    };

    if outcome == PlatformAccountOutcome::Saved {
        let _: () = request(
            "user.profile.save",
            UserProfileUpdate::unchanged(user.user_id)
                .with_platform_username(platform, username.clone()),
        )
        .await
        .map_err(internal)?;

        named.record(user.user_id).await;
    }

    let html = PlatformAccountForm {
        target_id: id_str,
        platform,
        visitor: PlatformAccountVisitor::SignedIn(username),
        outcome,
        has_app_links: !project.mobile_app_url.is_empty()
            || !project.mobile_app_url_android.is_empty()
            || !project.mobile_app_url_ios.is_empty(),
    }
    .render_once()
    .map_err(|e| internal(e.into()))?;

    Ok(Html(html).into_response())
}

#[derive(serde::Deserialize)]
pub struct InlineAuthQuery {
    mode: InlineAuthMode,
    #[serde(default)]
    email: String,
}

#[derive(serde::Deserialize)]
pub struct InlineAuthSubmission {
    mode: InlineAuthMode,
    email: String,
    password: String,
    newsletter: Option<bool>,
    #[serde(rename = "cf-turnstile-response")]
    cf_turnstile_response: Option<String>,
}

/// The id and page path of the project an inline sign-in form belongs to.
async fn inline_auth_project(id: &str) -> poem::Result<(Ulid, String)> {
    let project_id: Ulid = id
        .parse()
        .map_err(|_| poem::Error::from_status(poem::http::StatusCode::NOT_FOUND))?;
    let id_str = project_id.to_string();

    let slug = sqlx::query_scalar!(
        r#"SELECT slug FROM entities WHERE target = ? AND kind = 'Project'"#,
        id_str
    )
    .fetch_optional(db())
    .await
    .map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?
    .ok_or_else(|| poem::Error::from_status(poem::http::StatusCode::NOT_FOUND))?;

    Ok((project_id, project_page_path(&slug)))
}

fn render_inline_auth(form: InlineAuthForm) -> poem::Result<poem::Response> {
    let html = form.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(Html(html).into_response())
}

/// The page, reloaded signed in.
fn signed_in_response() -> poem::Response {
    poem::Response::builder()
        .header("HX-Refresh", "true")
        .finish()
}

/// Switch a project page's inline form between signing in and signing up,
/// keeping the email address typed so far.
#[handler]
pub async fn inline_auth_form(
    Path(id): Path<String>,
    session: &Session,
    Authenticated(user_info): Authenticated,
    Query(query): Query<InlineAuthQuery>,
) -> poem::Result<poem::Response> {
    if user_info.is_some() {
        return Ok(signed_in_response());
    }

    let (project_id, page_path) = inline_auth_project(&id).await?;

    render_inline_auth(InlineAuthForm {
        target_id: project_id.to_string(),
        page_path,
        mode: query.mode,
        email: query.email,
        offer_newsletter: !crate::auth::signup_will_be_protected(session),
        problem: None,
        pending_email: None,
    })
}

/// Sign in or create an account from a project page's inline form, joining
/// the project either way, and reload the page signed in. Anything short of
/// that shows the form again, saying why.
#[handler]
pub async fn inline_auth(
    Path(id): Path<String>,
    session: &Session,
    RealIp(client_ip): RealIp,
    Form(submission): Form<InlineAuthSubmission>,
) -> poem::Result<poem::Response> {
    use crate::auth::{
        PasswordLogin, PasswordSignup, SignupOutcome, log_in_with_password, sign_up_with_password,
    };

    let (project_id, page_path) = inline_auth_project(&id).await?;

    let form = |mode, problem, pending_email| InlineAuthForm {
        target_id: project_id.to_string(),
        page_path: page_path.clone(),
        mode,
        email: submission.email.clone(),
        offer_newsletter: !crate::auth::signup_will_be_protected(session),
        problem,
        pending_email,
    };

    match submission.mode {
        InlineAuthMode::SignIn => {
            match log_in_with_password(
                session,
                submission.email.clone(),
                submission.password,
                Some(page_path.clone()),
            )
            .await
            {
                PasswordLogin::SignedIn(basic_info) => {
                    // A signup joins as part of creating the account; signing
                    // in has to ask separately. Not fatal: they are signed in
                    // whether or not this lands.
                    if let Err(e) = request::<ProjectJoinResponse>(
                        "user.joins.create",
                        ProjectJoinRequest {
                            user_id: basic_info.user_id,
                            project_id: project_id.to_string(),
                        },
                    )
                    .await
                    {
                        tracing::warn!("Failed to join {project_id} on signing in: {e}");
                    }

                    Ok(signed_in_response())
                }
                PasswordLogin::Refused => render_inline_auth(form(
                    InlineAuthMode::SignIn,
                    Some(InlineAuthProblem::WrongPassword),
                    None,
                )),
                PasswordLogin::Unavailable => render_inline_auth(form(
                    InlineAuthMode::SignIn,
                    Some(InlineAuthProblem::Unavailable),
                    None,
                )),
            }
        }
        InlineAuthMode::SignUp => {
            if submission.password.chars().count() < MINIMUM_PASSWORD_LENGTH {
                return render_inline_auth(form(
                    InlineAuthMode::SignUp,
                    Some(InlineAuthProblem::PasswordTooShort),
                    None,
                ));
            }

            let fields = PasswordSignup {
                handle: String::new(),
                email: submission.email.clone(),
                password: submission.password,
                newsletter: submission.newsletter.unwrap_or(false),
                zipcode: String::new(),
                join_project: Some(project_id.to_string()),
            };

            let token = submission.cf_turnstile_response.as_deref().unwrap_or("");

            match sign_up_with_password(session, client_ip, fields, token, Some(page_path.clone()))
                .await
            {
                SignupOutcome::SignedIn(_) => Ok(signed_in_response()),
                SignupOutcome::Deferred => render_inline_auth(form(
                    InlineAuthMode::SignUp,
                    None,
                    Some(submission.email.clone()),
                )),
                SignupOutcome::AlreadyRegistered => render_inline_auth(form(
                    InlineAuthMode::SignIn,
                    Some(InlineAuthProblem::EmailTaken),
                    None,
                )),
                SignupOutcome::RateLimited => render_inline_auth(form(
                    InlineAuthMode::SignUp,
                    Some(InlineAuthProblem::RateLimited),
                    None,
                )),
                SignupOutcome::Unavailable => render_inline_auth(form(
                    InlineAuthMode::SignUp,
                    Some(InlineAuthProblem::Unavailable),
                    None,
                )),
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn platform_form() -> PlatformAccountForm {
        PlatformAccountForm {
            target_id: Ulid::new().to_string(),
            platform: LinkedPlatform::Zooniverse,
            visitor: PlatformAccountVisitor::SignedIn(String::new()),
            outcome: PlatformAccountOutcome::Unsubmitted,
            has_app_links: false,
        }
    }

    fn inline_auth_form() -> InlineAuthForm {
        InlineAuthForm {
            target_id: Ulid::new().to_string(),
            page_path: project_page_path("backyard-worlds"),
            mode: InlineAuthMode::SignUp,
            email: String::new(),
            offer_newsletter: true,
            problem: None,
            pending_email: None,
        }
    }

    fn inaturalist_form(visitor: PlatformAccountVisitor) -> PlatformAccountForm {
        PlatformAccountForm {
            platform: LinkedPlatform::INaturalist,
            visitor,
            ..platform_form()
        }
    }

    fn guidance(
        signed_in: bool,
        prefixes: (&str, &str),
        is_affiliate: bool,
        platform: bool,
    ) -> GetStartedGuidance {
        GetStartedGuidance::for_project(
            signed_in,
            prefixes.0.to_string(),
            prefixes.1.to_string(),
            is_affiliate,
            platform.then(platform_form),
        )
    }

    #[test]
    fn the_projects_own_guidance_replaces_everything_else() {
        let signed_in = guidance(true, ("<p>in</p>", "<p>out</p>"), true, true);
        assert!(matches!(signed_in, GetStartedGuidance::Custom(html) if html == "<p>in</p>"));

        let signed_out = guidance(false, ("<p>in</p>", "<p>out</p>"), true, true);
        assert!(matches!(signed_out, GetStartedGuidance::Custom(html) if html == "<p>out</p>"));
    }

    #[test]
    fn guidance_for_the_other_visitor_does_not_apply() {
        assert!(matches!(
            guidance(true, ("", "<p>out</p>"), true, true),
            GetStartedGuidance::LinkedPlatform(_)
        ));
    }

    #[test]
    fn a_platform_project_asks_for_the_account_ahead_of_the_email_advice() {
        assert!(matches!(
            guidance(true, ("", ""), true, true),
            GetStartedGuidance::LinkedPlatform(_)
        ));
        assert!(matches!(
            guidance(true, ("", ""), true, false),
            GetStartedGuidance::Affiliate
        ));
    }

    #[test]
    fn a_project_that_credits_nothing_adds_nothing() {
        assert!(guidance(true, ("", ""), false, false).is_nothing());
    }

    #[test]
    fn a_platform_where_only_some_projects_credit_says_so() {
        let html = PlatformAccountForm {
            visitor: PlatformAccountVisitor::SignedOut(inline_auth_form()),
            ..platform_form()
        }
        .render_once()
        .unwrap();

        assert!(html.contains("Three Easy Steps!"));
        assert!(html.contains("paste your Zooniverse username below (case-sensitive)."));
        assert!(html.contains("Sign up or sign in to Zooniverse</a> and copy your username."));
        assert!(html.contains("the name after the @ under your name on the Zooniverse home page."));
        assert!(html.contains("even if you join more SciStarter Affiliate Zooniverse projects"));
    }

    #[test]
    fn a_signed_out_visitor_is_walked_through_signing_in_first() {
        let html = inaturalist_form(PlatformAccountVisitor::SignedOut(inline_auth_form()))
            .render_once()
            .unwrap();

        assert!(html.contains("Three Easy Steps!"));
        assert!(html.contains("Sign in to SciStarter or create a free SciStarter account."));
        assert!(html.contains("class=\"inline-auth"));
        assert!(html.contains("<turnstile-widget"));
        assert!(html.contains(r#"name="username" type="text" disabled"#));
        assert!(html.contains("That's it!"));
    }

    #[test]
    fn a_signed_in_visitor_without_a_username_has_two_steps() {
        let html = inaturalist_form(PlatformAccountVisitor::SignedIn(String::new()))
            .render_once()
            .unwrap();

        assert!(html.contains("Two Easy Steps!"));
        assert!(!html.contains("inline-auth"));
        assert!(!html.contains("disabled"));
        assert!(!html.contains("case-sensitive"));
        assert!(!html.contains("app links"));
        assert!(html.contains("Sign up or sign in to iNaturalist</a> and copy your username.</p>"));
    }

    #[test]
    fn the_app_links_are_mentioned_only_when_the_page_has_them() {
        let html = PlatformAccountForm {
            has_app_links: true,
            ..inaturalist_form(PlatformAccountVisitor::SignedIn(String::new()))
        }
        .render_once()
        .unwrap();

        assert!(html.contains("Sign up or sign in to iNaturalist</a> (or use the app links below) and copy your username."));
    }

    #[test]
    fn a_connected_visitor_sees_their_username_and_may_change_it() {
        let html = PlatformAccountForm {
            outcome: PlatformAccountOutcome::Saved,
            ..inaturalist_form(PlatformAccountVisitor::SignedIn("Stargazer".to_string()))
        }
        .render_once()
        .unwrap();

        assert!(html.contains("Linked to iNaturalist account <strong>Stargazer</strong>"));
        assert!(html.contains("<details"));
        assert!(html.contains(r#"value="Stargazer""#));
        assert!(!html.contains("Easy Steps!"));
    }

    #[test]
    fn clearing_the_username_returns_to_the_steps() {
        let html = PlatformAccountForm {
            outcome: PlatformAccountOutcome::Blank,
            ..inaturalist_form(PlatformAccountVisitor::SignedIn(String::new()))
        }
        .render_once()
        .unwrap();

        assert!(html.contains("Two Easy Steps!"));
        assert!(html.contains("Please enter your iNaturalist username."));
    }

    #[test]
    fn a_login_zooniverse_does_not_have_returns_to_the_steps_saying_so() {
        let html = PlatformAccountForm {
            outcome: PlatformAccountOutcome::Unknown,
            visitor: PlatformAccountVisitor::SignedIn("Star Gazer".to_string()),
            ..platform_form()
        }
        .render_once()
        .unwrap();

        assert!(html.contains("Two Easy Steps!"));
        assert!(!html.contains("Linked to Zooniverse account"));
        assert!(html.contains("Zooniverse has no account named <strong>Star Gazer</strong>. It"));
        assert!(html.contains("the name after the @ under your name on the Zooniverse home page."));
    }

    #[test]
    fn the_inline_form_switches_between_signing_up_and_in() {
        let sign_up = inline_auth_form().render_once().unwrap();
        assert!(sign_up.contains(r#"name="mode" value="sign_up""#));
        assert!(sign_up.contains(r#"name="newsletter""#));
        assert!(sign_up.contains("?mode=sign_in"));

        let sign_in = InlineAuthForm {
            mode: InlineAuthMode::SignIn,
            email: "stargazer@example.invalid".to_string(),
            problem: Some(InlineAuthProblem::WrongPassword),
            ..inline_auth_form()
        }
        .render_once()
        .unwrap();
        assert!(sign_in.contains(r#"name="mode" value="sign_in""#));
        assert!(sign_in.contains(r#"value="stargazer@example.invalid""#));
        assert!(sign_in.contains("Incorrect email or password."));
        assert!(!sign_in.contains("<turnstile-widget"));
    }

    #[test]
    fn a_deferred_signup_says_to_check_email() {
        let html = InlineAuthForm {
            pending_email: Some("stargazer@example.invalid".to_string()),
            ..inline_auth_form()
        }
        .render_once()
        .unwrap();

        assert!(html.contains("Check your email."));
        assert!(!html.contains("<form"));
    }
}
