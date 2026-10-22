use std::{fmt::Display, sync::Arc};

use anyhow::Error;
use common::{
    message::{log::PageChange, user::BasicInfo},
    service::log_activity,
    shared::db,
    timestring::TimeString,
    transaction::Immediate,
};
use poem::{
    IntoResponse,
    http::{Method, StatusCode},
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

const DOMAIN: &str = env!("DOMAIN");

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
    session
        .get::<bool>(&entity_password_session_key(slug))
        .unwrap_or(false)
}

/// Grant password authorization for an entity in session
pub fn grant_password_authorization(session: &poem::session::Session, slug: &str) {
    session.set(&entity_password_session_key(slug), true);
}

/// Whether a request only reads, as opposed to submitting something.
fn is_read(method: &Method) -> bool {
    method == Method::GET || method == Method::HEAD
}

/// Permanently redirect a request to the address that replaced the one it
/// asked for.
///
/// A read gets a 301. Anything else gets a 308, because a browser following
/// a 301 is allowed to turn a POST into a GET and drop the form it carried.
fn moved(location: &str, req: &poem::Request) -> poem::Response {
    let location = forward_query(location, req.uri().query().unwrap_or(""));
    if is_read(req.method()) {
        Redirect::moved_permanent(location).into_response()
    } else {
        Redirect::permanent(location).into_response()
    }
}

/// Carry a request's query string over to a redirect's location, so that a
/// moved address keeps its parameters. They follow any the location already
/// has, and come before its fragment.
fn forward_query(location: &str, query: &str) -> String {
    if query.is_empty() {
        return location.to_string();
    }
    let (base, fragment) = match location.split_once('#') {
        Some((base, fragment)) => (base, Some(fragment)),
        None => (location, None),
    };
    let separator = if base.contains('?') { '&' } else { '?' };
    match fragment {
        Some(fragment) => format!("{base}{separator}{query}#{fragment}"),
        None => format!("{base}{separator}{query}"),
    }
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

impl EntityKind {
    /// Whether an entity of this kind is the address of a piece of hydra
    /// content, identified by its ULID. A piece of content has exactly one
    /// such address; the other kinds point at URLs or at nothing, and any
    /// number of them may share a target. The `entities_by_target` index
    /// enforces this, and must list the same kinds.
    pub fn addresses_content(&self) -> bool {
        match self {
            EntityKind::RawHtml
            | EntityKind::DressedHtml
            | EntityKind::Wysiwyg
            | EntityKind::PageBuilder
            | EntityKind::Form
            | EntityKind::Project
            | EntityKind::Event => true,
            EntityKind::Redirect | EntityKind::WordPress | EntityKind::Placeholder => false,
        }
    }

    /// The URL path prefix under which entities of this kind are
    /// served. Projects and events live under `/projects/` and
    /// `/events/`; every other kind is served at the top level.
    pub fn path_prefix(&self) -> &'static str {
        match self {
            EntityKind::Project => "/projects/",
            EntityKind::Event => "/events/",
            _ => "/",
        }
    }

    /// The path at which the entity of this kind with `slug` is served.
    pub fn path(&self, slug: &str) -> String {
        format!("{}{}", self.path_prefix(), slug)
    }
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

    /// Parse a target string into the `EntityTarget` variant expected
    /// for the given kind, returning an error if the string is not a
    /// valid value for that kind. An empty/whitespace string (or a kind
    /// that takes no target) yields `Nil`.
    pub fn parse_for_kind(kind: EntityKind, target: Option<&str>) -> Result<EntityTarget, Error> {
        match target {
            Some(s) if !s.trim().is_empty() => match kind {
                EntityKind::Redirect => {
                    // A leading-slash target is a relative same-site redirect
                    // (stored as a String); anything else must be an absolute
                    // URL. `Redirect::moved_permanent` accepts either form.
                    if s.starts_with('/') {
                        Ok(EntityTarget::String(s.to_string()))
                    } else {
                        Ok(EntityTarget::Url(s.parse::<Url>()?))
                    }
                }
                EntityKind::WordPress => Ok(EntityTarget::String(s.to_string())),
                EntityKind::RawHtml
                | EntityKind::DressedHtml
                | EntityKind::Wysiwyg
                | EntityKind::PageBuilder
                | EntityKind::Form
                | EntityKind::Project
                | EntityKind::Event => Ok(EntityTarget::Ulid(s.parse::<Ulid>()?)),
                EntityKind::Placeholder => Ok(EntityTarget::Nil),
            },
            _ => Ok(EntityTarget::Nil),
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
                EntityKind::Redirect => {
                    if value.target.starts_with('/') {
                        EntityTarget::String(value.target)
                    } else {
                        match value.target.parse() {
                            Ok(url) => EntityTarget::Url(url),
                            Err(_) => EntityTarget::Nil,
                        }
                    }
                }
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

    // Whoever may edit the page may see it unpublished.
    if let Some(user_info) = user_info {
        let editor = crate::edit_access::Editor::from(user_info);

        let may_edit = match (&entity.kind, &entity.target) {
            (EntityKind::Project, EntityTarget::Ulid(project_id)) => {
                crate::edit_access::may_edit(crate::edit_access::Page::Project(*project_id), editor)
                    .await?
            }
            (EntityKind::Event, EntityTarget::Ulid(event_id)) => {
                crate::edit_access::may_edit(crate::edit_access::Page::Event(*event_id), editor)
                    .await?
            }
            _ => editor.is_staff,
        };

        if may_edit {
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
    pub fn new_with_publish(
        slug: impl Into<String>,
        kind: EntityKind,
        target: EntityTarget,
        publish: bool,
    ) -> Entity {
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
    pub fn new_with_created(
        slug: impl Into<String>,
        kind: EntityKind,
        target: EntityTarget,
        publish: bool,
        created: TimeString,
    ) -> Entity {
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

    #[tracing::instrument(skip(session, req))]
    pub async fn response(
        &self,
        user_info: Option<Arc<BasicInfo>>,
        session: &poem::session::Session,
        req: &poem::Request,
    ) -> poem::Result<poem::Response> {
        let query = req.uri().query().unwrap_or("");

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
                // Redirect to an absolute URL or a relative same-site path.
                let location = match &self.target {
                    EntityTarget::Url(url) => Some(url.to_string()),
                    EntityTarget::String(path) => Some(path.clone()),
                    _ => None,
                };
                if let Some(location) = location {
                    moved(&location, req)
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

                    let layout = StandardLayout::new(self.canonical_path(), content_wrapper)
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

                    let layout = StandardLayout::new(self.canonical_path(), content_wrapper)
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
                    use crate::builder::context::{Context, ContextMode, RequestUrl};
                    use crate::pagebuilder::{PageBuilderPage, cache};

                    let page = PageBuilderPage::load(ulid)
                        .await
                        .map_err(|_| poem::Error::from_status(StatusCode::NOT_FOUND))?;

                    // Compile with caching (uses timestamp to detect stale cache)
                    let compiled = cache::compile_cached(ulid, &page.source, &page.updated)
                        .map_err(|e| {
                            poem::Error::from_string(
                                e.to_string(),
                                StatusCode::INTERNAL_SERVER_ERROR,
                            )
                        })?;

                    // Render the page in presenting mode
                    let ctx =
                        Context::new(db(), &user_info, ContextMode::Presenting).map_err(|e| {
                            poem::Error::from_string(
                                e.to_string(),
                                StatusCode::INTERNAL_SERVER_ERROR,
                            )
                        })?;
                    // Widgets on the page read their filter state from here, and
                    // aim their own forms back at the page with it.
                    ctx.set(RequestUrl {
                        path: self.canonical_path(),
                        query: query.to_string(),
                    })
                    .map_err(|e| {
                        poem::Error::from_string(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
                    })?;

                    let html = compiled.render(&ctx).await.map_err(|e| {
                        poem::Error::from_string(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
                    })?;

                    let content_wrapper = ContentPageContent {
                        content: Literal::new(&html),
                    };

                    let layout = StandardLayout::new(self.canonical_path(), content_wrapper)
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
            EntityKind::Project => {
                let target_id = self.target.as_ulid().ok_or_else(|| {
                    poem::Error::from_string(
                        "Project entity has no ULID target",
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                })?;

                let detail = crate::component::detail_page::DetailPage {
                    slug: self.slug.clone(),
                    kind: crate::component::detail_page::DetailPageKind::Project,
                    target_id,
                };

                let content = detail.prepare(user_info.clone()).await?;
                let title = content.name.clone();
                let can_edit = content.can_edit;
                let og_title = content.name.clone();
                let og_description = content.meta_description.clone();
                let og_image = content.image.clone();
                let og_url = format!("https://{}{}", DOMAIN, self.canonical_path());

                let layout = StandardLayout::new(self.canonical_path(), content)
                    .prepare(user_info.clone())
                    .await?;

                let mut head = Head::default().with_maplibre();
                if can_edit {
                    head = head.with_quill();
                }

                crate::component::detail_page::DetailPageWrapper {
                    head: head.prepare(None).await?,
                    layout,
                    title,
                    og_title,
                    og_description,
                    og_image,
                    og_url,
                }
                .into_response(None)
                .await?
            }
            EntityKind::Event => {
                let target_id = self.target.as_ulid().ok_or_else(|| {
                    poem::Error::from_string(
                        "Event entity has no ULID target",
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                })?;

                let detail = crate::component::detail_page::DetailPage {
                    slug: self.slug.clone(),
                    kind: crate::component::detail_page::DetailPageKind::Event,
                    target_id,
                };

                let content = detail.prepare(user_info.clone()).await?;
                let title = content.name.clone();
                let can_edit = content.can_edit;
                let og_title = content.name.clone();
                let og_description = content.meta_description.clone();
                let og_image = content.image.clone();
                let og_url = format!("https://{}{}", DOMAIN, self.canonical_path());

                let layout = StandardLayout::new(self.canonical_path(), content)
                    .prepare(user_info.clone())
                    .await?;

                let mut head = Head::default().with_maplibre();
                if can_edit {
                    head = head.with_quill();
                }

                crate::component::detail_page::DetailPageWrapper {
                    head: head.prepare(None).await?,
                    layout,
                    title,
                    og_title,
                    og_description,
                    og_image,
                    og_url,
                }
                .into_response(None)
                .await?
            }
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

                let layout = StandardLayout::new(self.canonical_path(), prepared_content)
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
                StandardLayout::new(self.canonical_path(), Placeholder)
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

        let layout = StandardLayout::new(self.canonical_path(), prompt)
            .prepare(user_info)
            .await?;

        Ok(EntityPasswordPage {
            head: Head::default().prepare(None).await?,
            layout,
        }
        .into_response(None)
        .await?)
    }

    #[tracing::instrument(skip(slug, session, req))]
    pub async fn response_for_slug(
        slug: impl AsRef<str>,
        prefix: &str,
        user_info: Option<Arc<BasicInfo>>,
        session: &poem::session::Session,
        req: &poem::Request,
    ) -> poem::Result<poem::Response> {
        // This as_ref().to_string() silliness is due to the issue
        // mentioned in peering.rs forcing us to use String instead of
        // &str as the key type.
        match Entity::load(slug.as_ref().to_string()).await {
            Ok(ent) => {
                // A Redirect forwards whatever request reaches it, so that
                // a form posting to a moved address still arrives. Every
                // other kind is a page, and only answers a read.
                //
                // If the entity was requested under a prefix that
                // isn't its canonical one (e.g. a project reached at
                // the top level, or a page reached under /projects/),
                // permanently redirect to the canonical path.
                if !is_read(req.method()) && !matches!(ent.kind(), EntityKind::Redirect) {
                    Ok(StatusCode::METHOD_NOT_ALLOWED.into())
                } else if prefix != ent.kind().path_prefix() {
                    Ok(moved(&ent.canonical_path(), req))
                } else {
                    ent.response(user_info, session, req).await
                }
            }
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

    /// The canonical public path for this entity. Projects and events
    /// live under `/projects/<slug>` and `/events/<slug>`; all other
    /// kinds are served at `/<slug>`.
    pub fn canonical_path(&self) -> String {
        self.kind.path(&self.slug)
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

    /// Create a copy of this entity with a different publication status
    pub fn with_publish(self, publish: bool) -> Entity {
        Entity { publish, ..self }
    }

    /// Publish the page of a project or event, keeping its slug and creation
    /// time. A target without a page yet gets one, its slug made from `name`.
    #[tracing::instrument(skip(name))]
    pub async fn publish_page(
        kind: EntityKind,
        target: Ulid,
        name: impl AsRef<str>,
    ) -> Result<(), Error> {
        let entity = match Entity::page_of(kind, target).await? {
            Some(page) => page.with_publish(true),
            None => Entity::new_by_name(name, kind, EntityTarget::Ulid(target)).await?,
        };

        entity.store().await?;
        log_activity(
            None,
            PageChange::EntityPublish,
            None,
            target,
            Some(entity.slug().to_string()),
        )
        .await
    }

    /// Unpublish the page of a project or event. A target without a page is
    /// unpublished already.
    #[tracing::instrument]
    pub async fn unpublish_page(kind: EntityKind, target: Ulid) -> Result<(), Error> {
        match Entity::page_of(kind, target).await? {
            Some(page) => page.with_publish(false).store().await,
            None => Ok(()),
        }
    }

    /// The page of a project or event as it stands, password included.
    pub async fn page_of(kind: EntityKind, target: Ulid) -> Result<Option<Entity>, Error> {
        let target_str = target.to_string();

        let Some(existing) = sqlx::query!(
            r#"SELECT slug, created, publish, password FROM entities WHERE kind = ? AND target = ?"#,
            kind,
            target_str
        )
        .fetch_optional(db())
        .await?
        else {
            return Ok(None);
        };

        Ok(Some(
            Entity::new_with_created(
                existing.slug,
                kind,
                EntityTarget::Ulid(target),
                existing.publish,
                TimeString::try_from(existing.created)?,
            )
            .with_password(existing.password),
        ))
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
        let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

        // Content has one address. If this entity gives its content a new
        // slug, the slug it had until now becomes a redirect to the new
        // address, so that links to it keep working. The kinds listed are
        // the ones `addresses_content` excludes, as in the index that
        // enforces the rule.
        if self.kind.addresses_content() {
            let canonical_path = self.canonical_path();
            sqlx::query!(
                r#"update "entities"
                   set "kind" = ?, "target" = ?, "password" = NULL
                   where "target" = ?
                     and "kind" not in ('Redirect', 'WordPress', 'Placeholder')
                     and LOWER("slug") <> LOWER(?)"#,
                EntityKind::Redirect,
                canonical_path,
                target,
                self.slug,
            )
            .execute(&mut tx)
            .await?;
        }

        sqlx::query!(
            r#"insert into "entities" (
                 "slug",
                 "kind",
                 "target",
                 "publish",
                 "created",
                 "password"
               )
               values (?, ?, ?, ?, ?, ?)
               on conflict (LOWER("slug")) do update set
                 "slug" = excluded."slug",
                 "kind" = excluded."kind",
                 "target" = excluded."target",
                 "publish" = excluded."publish",
                 "created" = excluded."created",
                 "password" = excluded."password"
            "#,
            self.slug,
            self.kind,
            target,
            self.publish,
            self.created,
            self.password,
        )
        .execute(&mut tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::forward_query;

    #[test]
    fn forward_query_leaves_a_bare_request_alone() {
        assert_eq!(forward_query("/add/project", ""), "/add/project");
    }

    #[test]
    fn forward_query_starts_a_query() {
        assert_eq!(
            forward_query("/add/project", "exchange=12"),
            "/add/project?exchange=12"
        );
    }

    #[test]
    fn forward_query_follows_the_targets_own_query() {
        assert_eq!(
            forward_query("/finder?kind[]=event", "near=Boston"),
            "/finder?kind[]=event&near=Boston"
        );
    }

    #[test]
    fn forward_query_keeps_the_fragment_last() {
        assert_eq!(forward_query("/page#success", "a=1"), "/page?a=1#success");
        assert_eq!(
            forward_query("https://example.org/page?x=2#top", "a=1"),
            "https://example.org/page?x=2&a=1#top"
        );
    }
}
