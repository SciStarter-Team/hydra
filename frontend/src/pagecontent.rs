use common::shared::db;
use ulid::Ulid;

use crate::peering::LoadStorePeered;

#[derive(
    Debug, Clone, Copy, serde::Serialize, serde::Deserialize, strum::AsRefStr, strum::EnumString,
)]
pub enum PageContentKind {
    RawHtml,
    DressedHtml,
    Wysiwyg,
    PageBuilder,
    PageBuilderTemplate,
    Form,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PageContent {
    pagecontent_id: Ulid,
    kind: PageContentKind,
    name: String,
    meta: String,
    content: Vec<u8>,
}

impl PageContent {
    pub fn new(kind: PageContentKind, name: impl Into<String>, content: &[u8]) -> PageContent {
        PageContent {
            pagecontent_id: Ulid::new(),
            kind,
            name: name.into(),
            meta: String::new(),
            content: content.into(),
        }
    }

    pub fn new_with_id(
        pagecontent_id: Ulid,
        kind: PageContentKind,
        name: impl Into<String>,
        content: &[u8],
    ) -> PageContent {
        PageContent {
            pagecontent_id,
            kind,
            name: name.into(),
            meta: String::new(),
            content: content.into(),
        }
    }

    pub fn kind(&self) -> PageContentKind {
        self.kind
    }

    pub fn set_kind(&mut self, kind: PageContentKind) {
        self.kind = kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    pub fn meta<T>(&self) -> Result<T, anyhow::Error>
    where
        T: for<'a> serde::Deserialize<'a>,
    {
        // We're using JSON encoding here instead of flexbuffers
        // because we might end up needing to run queries or
        // introspections on the meta field.
        Ok(serde_json::from_str(&self.meta)?)
    }

    pub fn set_meta(&mut self, meta: impl serde::Serialize) -> Result<(), anyhow::Error> {
        self.meta = serde_json::to_string(&meta)?;
        Ok(())
    }

    pub fn as_utf8(&self) -> Result<&str, anyhow::Error> {
        Ok(str::from_utf8(&self.content)?)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.content
    }
}

impl std::ops::Deref for PageContent {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.content
    }
}

impl std::ops::DerefMut for PageContent {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.content
    }
}

impl LoadStorePeered for PageContent {
    type DatabaseKey = (PageContentKind, Ulid);

    const GROUP: &str = "pagecontent";

    async fn save_to_database(&self) -> Result<(), anyhow::Error> {
        let pagecontent_id_string = self.pagecontent_id.to_string();
        let kind_string = self.kind.as_ref();

        sqlx::query!(
            r#"insert or replace into pagecontent ("pagecontent_id", "kind", "name", "meta", "content") values (?, ?, ?, ?, ?)"#,
            pagecontent_id_string,
            kind_string,
            self.name,
            self.meta,
            self.content,
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn read_from_database(key: Self::DatabaseKey) -> Result<Self, anyhow::Error> {
        struct Row {
            pagecontent_id: String,
            kind: String,
            name: String,
            meta: String,
            content: Vec<u8>,
        }

        let kind_string = key.0.as_ref();
        let ulid_string = key.1.to_string();

        let row = sqlx::query_as!(
            Row,
            r#"select "pagecontent_id", "kind", "name", "meta", "content" from pagecontent where "pagecontent_id" = ? and "kind" = ?"#,
            ulid_string,
            kind_string,
        )
        .fetch_one(db())
        .await?;

        Ok(PageContent {
            pagecontent_id: row.pagecontent_id.parse()?,
            kind: row.kind.parse()?,
            name: row.name,
            meta: row.meta,
            content: row.content,
        })
    }
}
