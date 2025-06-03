//! This module contains structs implementing simple Entity types,
//! such as redirects and unprocessed HTML pages. More complex entity
//! types have their own modules.
use common::shared::db;
use ulid::Ulid;

use crate::peering::LoadStorePeered;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Redirect {
    redirect_id: Ulid,
    target: String,
}

impl Redirect {
    pub fn new(target: impl Into<String>) -> Redirect {
        Redirect {
            redirect_id: Ulid::new(),
            target: target.into(),
        }
    }

    pub fn id(&self) -> Ulid {
        self.redirect_id
    }

    pub fn target(&self) -> &str {
        self.target.as_str()
    }
}

impl LoadStorePeered for Redirect {
    type DatabaseKey = Ulid;

    const GROUP: &str = "redirect";

    async fn save_to_database(&self) -> Result<(), anyhow::Error> {
        let redirect_id = self.redirect_id.to_string();

        sqlx::query!(
            r#"insert or replace into "redirects" ("redirect_id", "target")
            values (?, ?)"#,
            redirect_id,
            self.target
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn read_from_database(key: Self::DatabaseKey) -> Result<Self, anyhow::Error> {
        let redirect_id = key.to_string();

        struct RedirectDB {
            redirect_id: String,
            target: String,
        }

        let record = sqlx::query_as!(
            RedirectDB,
            r#"select
              "redirect_id",
              "target"
            from "redirects"
            where "redirect_id" = ?
            "#,
            redirect_id,
        )
        .fetch_one(db())
        .await?;

        Ok(Redirect {
            redirect_id: record.redirect_id.parse()?,
            target: record.target,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Html {
    pub html_id: Ulid,
    pub stylesheet_urls: Box<[String]>,
    pub script_urls: Box<[String]>,
    pub content: String,
}

impl Html {
    pub fn new(
        stylesheet_urls: impl IntoIterator<Item = impl AsRef<str>>,
        script_urls: impl IntoIterator<Item = impl AsRef<str>>,
        content: impl Into<String>,
    ) -> Html {
        Html {
            html_id: Ulid::new(),
            stylesheet_urls: stylesheet_urls
                .into_iter()
                .map(|s| s.as_ref().to_string())
                .collect(),
            script_urls: script_urls
                .into_iter()
                .map(|s| s.as_ref().to_string())
                .collect(),
            content: content.into(),
        }
    }

    pub fn id(&self) -> Ulid {
        self.html_id
    }

    pub fn has_stylesheets(&self) -> bool {
        !self.stylesheet_urls.is_empty()
    }

    pub fn has_scripts(&self) -> bool {
        !self.script_urls.is_empty()
    }

    pub fn stylesheets(&self) -> impl Iterator<Item = &str> {
        self.stylesheet_urls.iter().map(|s| s.as_str())
    }

    pub fn scripts(&self) -> impl Iterator<Item = &str> {
        self.script_urls.iter().map(|s| s.as_str())
    }

    pub fn content(&self) -> &str {
        self.content.as_str()
    }
}

impl LoadStorePeered for Html {
    type DatabaseKey = Ulid;

    const GROUP: &str = "html";

    async fn save_to_database(&self) -> Result<(), anyhow::Error> {
        let html_id = self.html_id.to_string();

        let mut tx = db().begin().await?;

        sqlx::query!(
            r#"insert or replace into "htmls" ("html_id", "content")
            values (?, ?)"#,
            html_id,
            self.content,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"delete from "html_stylesheet_urls" where "html_id" = ?"#,
            html_id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"delete from "html_script_urls" where "html_id" = ?"#,
            html_id
        )
        .execute(&mut *tx)
        .await?;

        for url in self.stylesheet_urls.iter() {
            sqlx::query!(
                r#"insert into "html_stylesheet_urls" ("html_id", "url") values (?, ?)"#,
                html_id,
                url,
            )
            .execute(&mut *tx)
            .await?;
        }

        for url in self.script_urls.iter() {
            sqlx::query!(
                r#"insert into "html_script_urls" ("html_id", "url") values (?, ?)"#,
                html_id,
                url,
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        Ok(())
    }

    async fn read_from_database(key: Self::DatabaseKey) -> Result<Self, anyhow::Error> {
        let html_id = key.to_string();

        struct HtmlDB {
            html_id: String,
            content: String,
        }

        let db = db();

        let record = sqlx::query_as!(
            HtmlDB,
            r#"select "html_id", "content" from "htmls" where "html_id" = ?"#,
            html_id,
        )
        .fetch_one(db)
        .await?;

        let stylesheet_urls = sqlx::query_scalar!(
            r#"select "url" from "html_stylesheet_urls" where "html_id" = ?"#,
            html_id
        )
        .fetch_all(db)
        .await?
        .into_boxed_slice();

        let script_urls = sqlx::query_scalar!(
            r#"select "url" from "html_script_urls" where "html_id" = ?"#,
            html_id
        )
        .fetch_all(db)
        .await?
        .into_boxed_slice();

        Ok(Html {
            html_id: record.html_id.parse()?,
            stylesheet_urls,
            script_urls,
            content: record.content,
        })
    }
}
