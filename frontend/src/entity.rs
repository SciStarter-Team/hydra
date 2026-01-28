use std::{fmt::Display, sync::Arc};

use anyhow::Error;
use common::{message::user::BasicInfo, shared::db, timestring::TimeString};
use poem::{
    IntoResponse,
    http::StatusCode,
    web::{Html, Redirect},
};
use ulid::Ulid;
use url::Url;

use crate::{
    component::{Component, Head, StandardLayout},
    misc::{Literal, Placeholder},
    pagecontent::{PageContent, PageContentKind},
    peering::LoadStorePeered,
};

/// Result of checking entity access permissions
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityAccessResult {
    /// Full access granted
    Granted,
    /// Entity has a password set and user needs to enter it
    PasswordRequired,
    /// No access (return 404)
    Denied,
}

/// Generate session key for password authorization of an entity
fn entity_password_session_key(slug: &str) -> String {
    format!("entity_pw_{}", slug.to_lowercase())
}

/// Check if a session has password authorization for an entity
pub fn is_password_authorized(session: &poem::session::Session, slug: &str) -> bool {
    session.get::<bool>(&entity_password_session_key(slug)).unwrap_or(false)
}

/// Grant password authorization for an entity in session
pub fn grant_password_authorization(session: &poem::session::Session, slug: &str) {
    session.set(&entity_password_session_key(slug), true);
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/content.stpl.html")]
struct ContentPage<'a> {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<ContentPageContent<'a>> as Component>::Prepared,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/entity_password.stpl.html")]
pub struct EntityPasswordPage {
    pub head: <Head as Component>::Prepared,
    pub layout: <StandardLayout<EntityPasswordPrompt> as Component>::Prepared,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/entity_password_prompt.stpl.html")]
pub struct EntityPasswordPrompt {
    pub slug: String,
    pub error: bool,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/content_page_content.stpl.html")]
struct ContentPageContent<'a> {
    content: Literal<'a>,
}

#[derive(
    Debug,
    Copy,
    Clone,
    sqlx::Type,
    serde::Serialize,
    serde::Deserialize,
    strum::AsRefStr,
    strum::EnumString,
)]
pub enum EntityKind {
    Redirect,
    RawHtml,
    DressedHtml,
    WordPress, // Used for publishing WordPress pages
    Wysiwyg,
    PageBuilder,
    Form,
    Project,
    Event,
    Placeholder,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, strum::AsRefStr)]
pub enum EntityTarget {
    Nil,
    Url(Url),
    Ulid(Ulid),
    String(String),
}

impl EntityTarget {
    pub fn as_url(&self) -> Option<&Url> {
        match self {
            EntityTarget::Url(url) => Some(url),
            _ => None,
        }
    }

    pub fn as_ulid(&self) -> Option<Ulid> {
        match self {
            EntityTarget::Ulid(ulid) => Some(*ulid),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            EntityTarget::String(string) => Some(string.as_ref()),
            _ => None,
        }
    }

    pub fn value_string(&self) -> String {
        match self {
            EntityTarget::Nil => String::new(),
            EntityTarget::Url(url) => url.to_string(),
            EntityTarget::Ulid(ulid) => ulid.to_string(),
            EntityTarget::String(string) => string.clone(),
        }
    }
}

impl Display for EntityTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntityTarget::Nil => write!(f, ""),
            EntityTarget::Url(url) => url.fmt(f),
            EntityTarget::Ulid(ulid) => ulid.fmt(f),
            EntityTarget::String(string) => string.fmt(f),
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Entity {
    slug: String,
    kind: EntityKind,
    target: EntityTarget,
    publish: bool,
    created: TimeString,
    password: Option<String>,
}

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/wordpress_entity.stpl.html")]
struct WordPressEntityPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<crate::component::wordpress::WordPressContentPrepared> as Component>::Prepared,
    title: String,
}

struct EntityDB {
    slug: String,
    kind: EntityKind,
    target: String,
    publish: bool,
    created: TimeString,
    password: Option<String>,
}

impl TryFrom<EntityDB> for Entity {
    type Error = anyhow::Error;

    fn try_from(value: EntityDB) -> Result<Self, Self::Error> {
        let target = if value.target.trim().is_empty() {
            EntityTarget::Nil
        } else {
            match value.kind {
                EntityKind::Redirect => match value.target.parse() {
                    Ok(url) => EntityTarget::Url(url),
                    Err(_) => EntityTarget::Nil,
                },
                EntityKind::WordPress => EntityTarget::String(value.target),
                EntityKind::RawHtml
                | EntityKind::DressedHtml
                | EntityKind::Wysiwyg
                | EntityKind::PageBuilder
                | EntityKind::Form
                | EntityKind::Project
                | EntityKind::Event => match value.target.parse() {
                    Ok(ulid) => EntityTarget::Ulid(ulid),
                    Err(_) => EntityTarget::Nil,
                },
                EntityKind::Placeholder => EntityTarget::Nil,
            }
        };

        Ok(Entity {
            slug: value.slug,
            kind: value.kind,
            target,
            publish: value.publish,
            created: value.created,
            password: value.password,
        })
    }
}

/// Check if a user can access an unpublished entity
/// Returns:
/// - Granted if user has full access
/// - PasswordRequired if entity has a password and user hasn't entered it yet
/// - Denied if no access (return 404)
async fn check_entity_access(
    entity: &Entity,
    user_info: Option<&BasicInfo>,
    password_authorized: bool,
) -> Result<EntityAccessResult, Error> {
    // Published entities are accessible to everyone
    if entity.publish {
        return Ok(EntityAccessResult::Granted);
    }

    // Check if user is staff/superuser
    if let Some(user_info) = user_info {
        if user_info.is_staff || user_info.is_superuser {
            return Ok(EntityAccessResult::Granted);
        }

        // Check if user is a page manager for this entity's target
        let is_manager = match (&entity.kind, &entity.target) {
            (EntityKind::Project, EntityTarget::Ulid(project_id)) => {
                let user_id_str = user_info.user_id.to_string();
                let project_id_str = project_id.to_string();

                sqlx::query_scalar!(
                    r#"SELECT EXISTS(
                        SELECT 1 FROM project_page_editors
                        WHERE project_id = ? AND user_id = ?
                    ) as "exists: bool""#,
                    project_id_str,
                    user_id_str
                )
                .fetch_one(db())
                .await?
            }
            (EntityKind::Event, EntityTarget::Ulid(event_id)) => {
                let user_id_str = user_info.user_id.to_string();
                let event_id_str = event_id.to_string();

                sqlx::query_scalar!(
                    r#"SELECT EXISTS(
                        SELECT 1 FROM event_page_editors
                        WHERE event_id = ? AND user_id = ?
                    ) as "exists: bool""#,
                    event_id_str,
                    user_id_str
                )
                .fetch_one(db())
                .await?
            }
            _ => false,
        };

        if is_manager {
            return Ok(EntityAccessResult::Granted);
        }
    }

    // User doesn't have role-based access. Check if entity has a password.
    if entity.password.is_some() {
        // If password has been authorized via session, grant access
        if password_authorized {
            return Ok(EntityAccessResult::Granted);
        }
        // Otherwise, prompt for password
        return Ok(EntityAccessResult::PasswordRequired);
    }

    // No password, no access
    Ok(EntityAccessResult::Denied)
}

impl Entity {
    #[tracing::instrument(skip(slug))]
    pub fn new(slug: impl Into<String>, kind: EntityKind, target: EntityTarget) -> Entity {
        Entity {
            slug: slug.into(),
            kind,
            target,
            publish: true,
            created: TimeString::now(),
            password: None,
        }
    }

    #[tracing::instrument(skip(slug))]
    pub fn with_publish(slug: impl Into<String>, kind: EntityKind, target: EntityTarget, publish: bool) -> Entity {
        Entity {
            slug: slug.into(),
            kind,
            target,
            publish,
            created: TimeString::now(),
            password: None,
        }
    }

    #[tracing::instrument(skip(slug))]
    pub fn with_created(slug: impl Into<String>, kind: EntityKind, target: EntityTarget, publish: bool, created: TimeString) -> Entity {
        Entity {
            slug: slug.into(),
            kind,
            target,
            publish,
            created,
            password: None,
        }
    }

    #[tracing::instrument(skip(name))]
    pub async fn new_by_name(
        name: impl AsRef<str>,
        kind: EntityKind,
        target: EntityTarget,
    ) -> Result<Entity, Error> {
        let name = name.as_ref();

        let base_slug = name
            .chars()
            .fold(String::with_capacity(name.len()), |mut s, c| {
                if c.is_ascii_alphanumeric() {
                    s.push(c.to_ascii_lowercase());
                    s
                } else if !s.ends_with('-') {
                    s.push('-');
                    s
                } else {
                    s
                }
            });

        let mut slug = base_slug.clone();
        let mut counter = 0u32;

        // Putting this in a transaction and storing a placeholder
        // makes collisions less likely, but it does not make them
        // impossible due to the distributed nature of the
        // system. Fortunately, the system can tolerate occasinal
        // collisions, especially since they're confined to entities
        // with the same ASCII representation of their name and are
        // probably the result of bouncy inputs.
        let mut tx = db().begin().await?;

        while sqlx::query_scalar!(
            r#"select exists(select 1 from "entities" where LOWER("slug") = LOWER(?)) as "conflict: bool""#,
            slug
        )
        .fetch_one(&mut *tx)
        .await?
        .unwrap_or(false)
        {
            counter += 1;
            slug = format!("{}-{}", &base_slug, counter);
        }

        sqlx::query!(
            r#"insert into "entities" ("slug", "kind", "target", "created") values (?, ?, '00000000000000000000000000', datetime('now'))"#,
            slug,
            EntityKind::Placeholder,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(Entity::new(slug, kind, target))
    }

    #[tracing::instrument(skip(session))]
    pub async fn response(
        &self,
        user_info: Option<Arc<BasicInfo>>,
        session: &poem::session::Session,
    ) -> poem::Result<poem::Response> {
        // Check access permissions for unpublished entities
        let password_authorized = is_password_authorized(session, &self.slug);
        match check_entity_access(self, user_info.as_deref(), password_authorized).await? {
            EntityAccessResult::Granted => {}
            EntityAccessResult::PasswordRequired => {
                // Show password prompt page
                return self.password_prompt_response(user_info, false).await;
            }
            EntityAccessResult::Denied => {
                return Err(poem::Error::from_status(StatusCode::NOT_FOUND));
            }
        }

        Ok(match self.kind {
            EntityKind::Redirect => {
                if let Some(url) = self.target.as_url() {
                    Redirect::moved_permanent(url).into_response()
                } else {
                    return Err(poem::Error::from_status(StatusCode::NOT_FOUND));
                }
            }
            EntityKind::RawHtml => {
                if let Some(ulid) = self.target.as_ulid() {
                    let content = PageContent::load((PageContentKind::RawHtml, ulid)).await?;
                    Html(content.as_utf8()?).into_response()
                } else {
                    return Err(poem::Error::from_status(StatusCode::NOT_FOUND));
                }
            }
            EntityKind::DressedHtml => {
                if let Some(ulid) = self.target.as_ulid() {
                    let content = PageContent::load((PageContentKind::DressedHtml, ulid)).await?;

                    let content_wrapper = ContentPageContent {
                        content: Literal::new(content.as_utf8()?),
                    };

                    let layout = StandardLayout::new(content_wrapper)
                        .prepare(user_info.clone())
                        .await?;

                    ContentPage {
                        head: Head::default().prepare(None).await?,
                        layout,
                    }
                    .into_response(None)
                    .await?
                } else {
                    return Err(poem::Error::from_status(StatusCode::NOT_FOUND));
                }
            }
            EntityKind::Wysiwyg => {
                if let Some(ulid) = self.target.as_ulid() {
                    let content = PageContent::load((PageContentKind::Wysiwyg, ulid)).await?;
                    // Convert Quill Delta to HTML
                    let html = common::quill::delta_to_html(content.as_utf8()?)?;

                    let content_wrapper = ContentPageContent {
                        content: Literal::new(&html),
                    };

                    let layout = StandardLayout::new(content_wrapper)
                        .prepare(user_info.clone())
                        .await?;

                    ContentPage {
                        head: Head::default().prepare(None).await?,
                        layout,
                    }
                    .into_response(None)
                    .await?
                } else {
                    return Err(poem::Error::from_status(StatusCode::NOT_FOUND));
                }
            }
            EntityKind::PageBuilder => {
                if let Some(ulid) = self.target.as_ulid() {
                    use crate::builder::context::{Context, ContextMode};
                    use crate::pagebuilder::{PageBuilderPage, cache};

                    let page = PageBuilderPage::load(ulid).await.map_err(|_| {
                        poem::Error::from_status(StatusCode::NOT_FOUND)
                    })?;

                    // Compile with caching (uses timestamp to detect stale cache)
                    let compiled = cache::compile_cached(ulid, &page.source, &page.updated)
                        .map_err(|e| poem::Error::from_string(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR))?;

                    // Render the page in presenting mode
                    let ctx = Context::new(db(), &user_info, ContextMode::Presenting)
                        .map_err(|e| poem::Error::from_string(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR))?;
                    let html = compiled.render(&ctx).await
                        .map_err(|e| poem::Error::from_string(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR))?;

                    let content_wrapper = ContentPageContent {
                        content: Literal::new(&html),
                    };

                    let layout = StandardLayout::new(content_wrapper)
                        .prepare(user_info.clone())
                        .await?;

                    ContentPage {
                        head: Head::default().prepare(None).await?,
                        layout,
                    }
                    .into_response(None)
                    .await?
                } else {
                    return Err(poem::Error::from_status(StatusCode::NOT_FOUND));
                }
            }
            EntityKind::Form => todo!(),
            EntityKind::Project => todo!(),
            EntityKind::Event => todo!(),
            EntityKind::WordPress => {
                let page_url = self.target.as_str().ok_or_else(|| {
                    poem::Error::from_string(
                        "WordPress entity has no target URL",
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                })?;

                let wordpress_content = crate::component::WordPressContent {
                    page_url: page_url.to_string(),
                };

                let prepared_content = wordpress_content.prepare(user_info.clone()).await?;
                let title = prepared_content.title.clone();

                let layout = StandardLayout::new(prepared_content)
                    .prepare(user_info.clone())
                    .await?;

                WordPressEntityPage {
                    head: Head::default().prepare(None).await?,
                    layout,
                    title,
                }
                .into_response(None)
                .await?
            }
            EntityKind::Placeholder => {
                StandardLayout::new(Placeholder)
                    .into_response(user_info)
                    .await?
            }
        })
    }

    /// Render the password prompt page for this entity
    #[tracing::instrument]
    pub async fn password_prompt_response(
        &self,
        user_info: Option<Arc<BasicInfo>>,
        error: bool,
    ) -> poem::Result<poem::Response> {
        let prompt = EntityPasswordPrompt {
            slug: self.slug.clone(),
            error,
        };

        let layout = StandardLayout::new(prompt)
            .prepare(user_info)
            .await?;

        Ok(EntityPasswordPage {
            head: Head::default().prepare(None).await?,
            layout,
        }
        .into_response(None)
        .await?)
    }

    #[tracing::instrument(skip(slug, session))]
    pub async fn response_for_slug(
        slug: impl AsRef<str>,
        user_info: Option<Arc<BasicInfo>>,
        session: &poem::session::Session,
    ) -> poem::Result<poem::Response> {
        // This as_ref().to_string() silliness is due to the issue
        // mentioned in peering.rs forcing us to use String instead of
        // &str as the key type.
        match Entity::load(slug.as_ref().to_string()).await {
            Ok(ent) => ent.response(user_info, session).await,
            Err(err) => match err.downcast_ref::<sqlx::Error>() {
                Some(sqlx_err) => match sqlx_err {
                    sqlx::Error::RowNotFound => Ok(StatusCode::NOT_FOUND.into()),
                    _ => Err(err.into()),
                },
                None => Err(err.into()),
            },
        }
    }

    #[tracing::instrument]
    pub fn slug(&self) -> &str {
        &self.slug
    }

    pub fn kind(&self) -> &EntityKind {
        &self.kind
    }

    pub fn target(&self) -> &EntityTarget {
        &self.target
    }

    pub fn publish(&self) -> bool {
        self.publish
    }

    pub fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    pub fn created(&self) -> &TimeString {
        &self.created
    }

    /// Create a copy of this entity with a different password
    pub fn with_password(self, password: Option<String>) -> Entity {
        Entity { password, ..self }
    }

    /// Create a copy of this entity with a different kind
    pub fn with_kind(self, kind: EntityKind) -> Entity {
        Entity { kind, ..self }
    }

    /// Create a copy of this entity with a different target
    pub fn with_target(self, target: EntityTarget) -> Entity {
        Entity { target, ..self }
    }

    /// Check if an entity slug exists in the database (case-insensitive)
    /// More efficient than loading the full object for uniqueness checks
    #[tracing::instrument]
    pub async fn slug_exists(slug: &str) -> Result<bool, Error> {
        let row = sqlx::query!(
            r#"SELECT "slug" FROM "entities" WHERE LOWER("slug") = LOWER(?) LIMIT 1"#,
            slug
        )
        .fetch_optional(db())
        .await?;

        Ok(row.is_some())
    }
}

impl LoadStorePeered for Entity {
    //type DatabaseKey<'k> = &'k str;
    type DatabaseKey = String;

    const GROUP: &str = "entity";

    //async fn read_from_database<'k>(slug: Self::DatabaseKey<'k>) -> Result<Self, Error> {
    #[tracing::instrument]
    async fn read_from_database(slug: Self::DatabaseKey) -> Result<Self, Error> {
        Ok(sqlx::query_as!(
            EntityDB,
            r#"select
                 "slug",
                 "kind" as "kind: EntityKind",
                 "target",
                 "publish",
                 "created" as "created: TimeString",
                 "password"
               from "entities"
               where LOWER("slug") = LOWER(?)"#,
            slug,
        )
        .fetch_one(db())
        .await?
        .try_into()?)
    }

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        let target = self.target.to_string();

        // Tried using the standard "on conflict" clause, but it
        // handles the case when both unique columns conflict with
        // different rows poorly. The nonstandard "insert or replace",
        // on the other hand, handles that case cleanly, and does
        // precisely what we want it to as well.

        // r#"insert into "entities" (
        //      "slug",
        //      "kind",
        //      "target"
        //    )
        //    values (
        //      ?1,
        //      ?2,
        //      ?3
        //    )
        //    on conflict ("slug") do update set
        //      "kind" = excluded."kind",
        //      "target" = excluded."target"
        //    on conflict ("target") do update set
        //      "slug" = excluded."slug",
        //      "kind" = excluded."kind"
        // "#,

        sqlx::query!(
            r#"insert or replace into "entities" (
                 "slug",
                 "kind",
                 "target",
                 "publish",
                 "created",
                 "password"
               )
               values (
                 ?1,
                 ?2,
                 ?3,
                 ?4,
                 ?5,
                 ?6
               )
            "#,
            self.slug,
            self.kind,
            target,
            self.publish,
            self.created,
            self.password,
        )
        .execute(db())
        .await?;

        Ok(())
    }
}
