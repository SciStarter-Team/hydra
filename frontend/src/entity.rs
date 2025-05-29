use anyhow::Error;
use common::shared::{db, nats};
use futures::StreamExt;
use poem::{IntoResponse, web::Html};
use ulid::Ulid;

use crate::peering::LoadStorePeered;

#[derive(sqlx::Type, serde::Serialize, serde::Deserialize)]
pub enum EntityKind {
    Redirect,
    RawHTML,
    Wysiwyg,
    PageBuilder,
    Form,
    Project,
    Event,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Entity {
    slug: String,
    kind: EntityKind,
    target: Ulid,
}

struct EntityDB {
    slug: String,
    kind: EntityKind,
    target: String,
}

impl TryFrom<EntityDB> for Entity {
    type Error = ulid::DecodeError;

    fn try_from(value: EntityDB) -> Result<Self, Self::Error> {
        Ok(Entity {
            slug: value.slug,
            kind: value.kind,
            target: value.target.parse()?,
        })
    }
}

impl Entity {
    pub async fn lookup(slug: impl AsRef<str>) -> Result<Entity, Error> {
        Ok(Entity {
            slug: slug.as_ref().to_string(),
            kind: EntityKind::RawHTML,
            target: Ulid::nil(),
        })
    }

    pub async fn build_response(&self) -> poem::Result<poem::Response> {
        Ok(Html(format!("Viewing entity `{}`", &self.slug)).into_response())
    }

    pub async fn lookup_response(slug: impl AsRef<str>) -> poem::Result<poem::Response> {
        Entity::lookup(slug).await?.build_response().await
    }
}

impl LoadStorePeered for Entity {
    type DatabaseKey = String;

    const GROUP: &str = "entity";

    async fn read_from_database(slug: &Self::DatabaseKey) -> Result<Self, Error> {
        Ok(sqlx::query_as!(
            EntityDB,
            r#"select
                 "slug",
                 "kind" as "kind: EntityKind",
                 "target"
               from "entities"
               where "slug" = ?"#,
            slug,
        )
        .fetch_one(db())
        .await?
        .try_into()?)
    }

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
                 "target"
               )
               values (
                 ?1,
                 ?2,
                 ?3
               )
            "#,
            self.slug,
            self.kind,
            target,
        )
        .execute(db())
        .await?;

        Ok(())
    }
}
