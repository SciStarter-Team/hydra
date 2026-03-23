use anyhow::Error;
use common::shared::db;
use poem::{handler, web::Path};
use ulid::Ulid;

use crate::peering::LoadStorePeered;

#[derive(
    Debug,
    Copy,
    Clone,
    sqlx::Type,
    serde::Serialize,
    serde::Deserialize,
    strum::AsRefStr,
    strum::EnumString,
    strum::Display,
    PartialEq,
)]
#[sqlx(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum NewsItemSource {
    WordPress,
    Custom,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NewsItem {
    pub news_id: Ulid,
    pub source_type: NewsItemSource,
    pub tag: String,
    pub title: String,
    pub content: String,
    pub author: String,
    pub display_date: String,
    pub url: String,
    pub active: bool,
    pub sort_order: i32,
}

struct NewsItemDB {
    news_id: String,
    source_type: NewsItemSource,
    tag: String,
    title: String,
    content: String,
    author: String,
    display_date: String,
    url: String,
    active: i64,
    sort_order: i64,
}

impl TryFrom<NewsItemDB> for NewsItem {
    type Error = anyhow::Error;

    fn try_from(value: NewsItemDB) -> Result<Self, Self::Error> {
        Ok(NewsItem {
            news_id: value.news_id.parse()?,
            source_type: value.source_type,
            tag: value.tag,
            title: value.title,
            content: value.content,
            author: value.author,
            display_date: value.display_date,
            url: value.url,
            active: value.active != 0,
            sort_order: value.sort_order as i32,
        })
    }
}

pub struct RenderedNewsItem {
    pub news_id: Ulid,
    pub title: String,
    pub tag: String,
    pub html_content: String,
    pub author: String,
    pub display_date: String,
    pub url: String,
    pub source_type: NewsItemSource,
}

impl NewsItem {
    pub async fn get_active_items() -> Result<Vec<NewsItem>, Error> {
        let items_raw = sqlx::query_as!(
            NewsItemDB,
            r#"SELECT news_id,
                      source_type as "source_type: NewsItemSource",
                      tag,
                      title,
                      content,
                      author,
                      display_date,
                      url,
                      active,
                      sort_order
               FROM news_items
               WHERE active = 1
               ORDER BY sort_order ASC, display_date DESC"#,
        )
        .fetch_all(db())
        .await?;

        items_raw
            .into_iter()
            .map(|row| row.try_into())
            .collect::<Result<Vec<_>, _>>()
    }

    pub async fn render(&self) -> Result<RenderedNewsItem, Error> {
        match self.source_type {
            NewsItemSource::WordPress => {
                let wp = crate::wordpress::fetch_page_with_styles(&self.content).await?;
                Ok(RenderedNewsItem {
                    news_id: self.news_id,
                    title: if self.title.is_empty() {
                        wp.title
                    } else {
                        self.title.clone()
                    },
                    tag: self.tag.clone(),
                    html_content: wp.content,
                    author: self.author.clone(),
                    display_date: self.display_date.clone(),
                    url: self.url.clone(),
                    source_type: self.source_type,
                })
            }
            NewsItemSource::Custom => {
                let html = common::quill::delta_to_html(&self.content).unwrap_or_default();
                Ok(RenderedNewsItem {
                    news_id: self.news_id,
                    title: self.title.clone(),
                    tag: self.tag.clone(),
                    html_content: html,
                    author: self.author.clone(),
                    display_date: self.display_date.clone(),
                    url: self.url.clone(),
                    source_type: self.source_type,
                })
            }
        }
    }

    pub async fn get_rendered_active_items() -> Vec<RenderedNewsItem> {
        let items = match Self::get_active_items().await {
            Ok(items) => items,
            Err(e) => {
                tracing::error!("Failed to get active news items: {}", e);
                return Vec::new();
            }
        };

        let mut rendered = Vec::with_capacity(items.len());
        for item in items {
            match item.render().await {
                Ok(r) => rendered.push(r),
                Err(e) => {
                    tracing::error!("Failed to render news item {}: {}", item.news_id, e);
                }
            }
        }
        rendered
    }
}

#[handler]
pub async fn news_content(Path(id_str): Path<String>) -> Result<poem::Response, anyhow::Error> {
    let news_id: Ulid = id_str.parse()?;
    let item = NewsItem::load(news_id).await?;

    if !item.active {
        return Err(anyhow::anyhow!("News item not found"));
    }

    let rendered = item.render().await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(rendered.html_content))
}

impl LoadStorePeered for NewsItem {
    type DatabaseKey = Ulid;

    const GROUP: &str = "news";

    #[tracing::instrument]
    async fn read_from_database(news_id: Self::DatabaseKey) -> Result<Self, Error> {
        let news_id_str = news_id.to_string();
        Ok(sqlx::query_as!(
            NewsItemDB,
            r#"SELECT news_id,
                      source_type as "source_type: NewsItemSource",
                      tag,
                      title,
                      content,
                      author,
                      display_date,
                      url,
                      active,
                      sort_order
               FROM news_items
               WHERE news_id = ?"#,
            news_id_str,
        )
        .fetch_one(db())
        .await?
        .try_into()?)
    }

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        let news_id_str = self.news_id.to_string();
        let active_i64 = if self.active { 1i64 } else { 0i64 };
        let sort_order_i64 = self.sort_order as i64;

        sqlx::query!(
            r#"INSERT OR REPLACE INTO news_items (
                 news_id,
                 source_type,
                 tag,
                 title,
                 content,
                 author,
                 display_date,
                 url,
                 active,
                 sort_order
               )
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
            news_id_str,
            self.source_type,
            self.tag,
            self.title,
            self.content,
            self.author,
            self.display_date,
            self.url,
            active_i64,
            sort_order_i64,
        )
        .execute(db())
        .await?;

        Ok(())
    }

    #[tracing::instrument]
    async fn delete_from_database(news_id: Self::DatabaseKey) -> Result<(), Error> {
        let news_id_str = news_id.to_string();
        sqlx::query!(r#"DELETE FROM news_items WHERE news_id = ?"#, news_id_str)
            .execute(db())
            .await?;
        Ok(())
    }
}
