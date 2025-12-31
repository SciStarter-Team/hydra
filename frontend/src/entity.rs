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

#[derive(sailfish::TemplateOnce)]
#[template(path = "pages/content.stpl.html")]
struct ContentPage<'a> {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<ContentPageContent<'a>> as Component>::Prepared,
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
        })
    }
}

/// Check if a user can access an unpublished entity
/// Returns true if:
/// - Entity is published (everyone can access)
/// - User is staff/superuser (can access all unpublished entities)
/// - User is a page manager for the entity's target (project/event)
async fn can_user_access_entity(entity: &Entity, user_info: Option<&BasicInfo>) -> Result<bool, Error> {
    // Published entities are accessible to everyone
    if entity.publish {
        return Ok(true);
    }

    // Unpublished entities require authentication
    let Some(user_info) = user_info else {
        return Ok(false);
    };

    // Staff and superusers can access all unpublished entities
    if user_info.is_staff || user_info.is_superuser {
        return Ok(true);
    }

    // Check if user is a page manager for this entity's target
    match (&entity.kind, &entity.target) {
        (EntityKind::Project, EntityTarget::Ulid(project_id)) => {
            let user_id_str = user_info.user_id.to_string();
            let project_id_str = project_id.to_string();

            let is_manager = sqlx::query_scalar!(
                r#"SELECT EXISTS(
                    SELECT 1 FROM project_page_editors
                    WHERE project_id = ? AND user_id = ?
                ) as "exists: bool""#,
                project_id_str,
                user_id_str
            )
            .fetch_one(db())
            .await?;

            Ok(is_manager)
        }
        (EntityKind::Event, EntityTarget::Ulid(event_id)) => {
            let user_id_str = user_info.user_id.to_string();
            let event_id_str = event_id.to_string();

            let is_manager = sqlx::query_scalar!(
                r#"SELECT EXISTS(
                    SELECT 1 FROM event_page_editors
                    WHERE event_id = ? AND user_id = ?
                ) as "exists: bool""#,
                event_id_str,
                user_id_str
            )
            .fetch_one(db())
            .await?;

            Ok(is_manager)
        }
        // Other entity kinds don't have page managers, only staff can access
        _ => Ok(false),
    }
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

    #[tracing::instrument]
    pub async fn response(
        &self,
        user_info: Option<Arc<BasicInfo>>,
    ) -> poem::Result<poem::Response> {
        // Check access permissions for unpublished entities
        if !can_user_access_entity(self, user_info.as_deref()).await? {
            return Err(poem::Error::from_status(StatusCode::NOT_FOUND));
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

    #[tracing::instrument(skip(slug))]
    pub async fn response_for_slug(
        slug: impl AsRef<str>,
        user_info: Option<Arc<BasicInfo>>,
    ) -> poem::Result<poem::Response> {
        // This as_ref().to_string() silliness is due to the issue
        // mentioned in peering.rs forcing us to use String instead of
        // &str as the key type.
        match Entity::load(slug.as_ref().to_string()).await {
            Ok(ent) => ent.response(user_info).await,
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
                 "created" as "created: TimeString"
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
                 "created"
               )
               values (
                 ?1,
                 ?2,
                 ?3,
                 ?4,
                 ?5
               )
            "#,
            self.slug,
            self.kind,
            target,
            self.publish,
            self.created,
        )
        .execute(db())
        .await?;

        Ok(())
    }
}
