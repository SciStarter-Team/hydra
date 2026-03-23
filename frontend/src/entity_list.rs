use anyhow::Error;
use common::shared::db;
use ulid::Ulid;

use crate::peering::LoadStorePeered;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EntityList {
    pub list_id: Ulid,
    pub created_at: String,
    pub private: bool,
    pub owner_id: Ulid,
    pub delegate_id: Option<Ulid>,
    pub name: String,
    pub note: String,
    pub sidebar: String,
    pub slug: Option<String>,
    pub hide_identity_form: bool,
    pub image: String,
}

struct EntityListDB {
    list_id: String,
    created_at: String,
    private: i64,
    owner_id: String,
    delegate_id: Option<String>,
    name: String,
    note: String,
    sidebar: String,
    slug: Option<String>,
    hide_identity_form: i64,
    image: String,
}

impl TryFrom<EntityListDB> for EntityList {
    type Error = anyhow::Error;

    fn try_from(value: EntityListDB) -> Result<Self, Self::Error> {
        Ok(EntityList {
            list_id: value.list_id.parse()?,
            created_at: value.created_at,
            private: value.private != 0,
            owner_id: value.owner_id.parse()?,
            delegate_id: value.delegate_id.and_then(|s| s.parse().ok()),
            name: value.name,
            note: value.note,
            sidebar: value.sidebar,
            slug: value.slug,
            hide_identity_form: value.hide_identity_form != 0,
            image: value.image,
        })
    }
}

impl EntityList {
    pub async fn get_by_slug(slug: &str) -> Result<EntityList, Error> {
        Ok(sqlx::query_as!(
            EntityListDB,
            r#"SELECT list_id, created_at, private, owner_id, delegate_id, name, note, sidebar, slug, hide_identity_form, image
               FROM entity_lists
               WHERE slug = ?"#,
            slug,
        )
        .fetch_one(db())
        .await?
        .try_into()?)
    }

    pub async fn get_user_lists(owner_id: Ulid) -> Result<Vec<EntityList>, Error> {
        let owner_id_str = owner_id.to_string();
        let items = sqlx::query_as!(
            EntityListDB,
            r#"SELECT list_id, created_at, private, owner_id, delegate_id, name, note, sidebar, slug, hide_identity_form, image
               FROM entity_lists
               WHERE owner_id = ? OR delegate_id = ?
               ORDER BY name ASC"#,
            owner_id_str,
            owner_id_str,
        )
        .fetch_all(db())
        .await?;

        items
            .into_iter()
            .map(|row| row.try_into())
            .collect::<Result<Vec<_>, _>>()
    }
}

impl LoadStorePeered for EntityList {
    type DatabaseKey = Ulid;

    const GROUP: &str = "entity-list";

    #[tracing::instrument]
    async fn read_from_database(list_id: Self::DatabaseKey) -> Result<Self, Error> {
        let list_id_str = list_id.to_string();
        Ok(sqlx::query_as!(
            EntityListDB,
            r#"SELECT list_id, created_at, private, owner_id, delegate_id, name, note, sidebar, slug, hide_identity_form, image
               FROM entity_lists
               WHERE list_id = ?"#,
            list_id_str,
        )
        .fetch_one(db())
        .await?
        .try_into()?)
    }

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        let list_id_str = self.list_id.to_string();
        let owner_id_str = self.owner_id.to_string();
        let delegate_id_str = self.delegate_id.map(|u| u.to_string());
        let private_i64 = if self.private { 1i64 } else { 0i64 };
        let hide_identity_form_i64 = if self.hide_identity_form { 1i64 } else { 0i64 };

        sqlx::query!(
            r#"INSERT OR REPLACE INTO entity_lists (
                 list_id, created_at, private, owner_id, delegate_id, name, note, sidebar, slug, hide_identity_form, image
               )
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
            list_id_str,
            self.created_at,
            private_i64,
            owner_id_str,
            delegate_id_str,
            self.name,
            self.note,
            self.sidebar,
            self.slug,
            hide_identity_form_i64,
            self.image,
        )
        .execute(db())
        .await?;

        Ok(())
    }

    #[tracing::instrument]
    async fn delete_from_database(list_id: Self::DatabaseKey) -> Result<(), Error> {
        let list_id_str = list_id.to_string();
        sqlx::query!(r#"DELETE FROM entity_lists WHERE list_id = ?"#, list_id_str)
            .execute(db())
            .await?;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EntityListEntry {
    pub entry_id: Ulid,
    pub list_id: Ulid,
    pub priority: i32,
    pub added_at: String,
    pub entity_slug: String,
    pub note: String,
    pub link: String,
}

struct EntityListEntryDB {
    entry_id: String,
    list_id: String,
    priority: i64,
    added_at: String,
    entity_slug: String,
    note: String,
    link: String,
}

impl TryFrom<EntityListEntryDB> for EntityListEntry {
    type Error = anyhow::Error;

    fn try_from(value: EntityListEntryDB) -> Result<Self, Self::Error> {
        Ok(EntityListEntry {
            entry_id: value.entry_id.parse()?,
            list_id: value.list_id.parse()?,
            priority: value.priority as i32,
            added_at: value.added_at,
            entity_slug: value.entity_slug,
            note: value.note,
            link: value.link,
        })
    }
}

impl EntityListEntry {
    pub async fn get_entries(list_id: Ulid) -> Result<Vec<EntityListEntry>, Error> {
        let list_id_str = list_id.to_string();
        let items = sqlx::query_as!(
            EntityListEntryDB,
            r#"SELECT entry_id, list_id, priority, added_at, entity_slug, note, link
               FROM entity_list_entries
               WHERE list_id = ?
               ORDER BY priority ASC, added_at ASC"#,
            list_id_str,
        )
        .fetch_all(db())
        .await?;

        items
            .into_iter()
            .map(|row| row.try_into())
            .collect::<Result<Vec<_>, _>>()
    }
}

impl LoadStorePeered for EntityListEntry {
    type DatabaseKey = Ulid;

    const GROUP: &str = "entity-list-entry";

    #[tracing::instrument]
    async fn read_from_database(entry_id: Self::DatabaseKey) -> Result<Self, Error> {
        let entry_id_str = entry_id.to_string();
        Ok(sqlx::query_as!(
            EntityListEntryDB,
            r#"SELECT entry_id, list_id, priority, added_at, entity_slug, note, link
               FROM entity_list_entries
               WHERE entry_id = ?"#,
            entry_id_str,
        )
        .fetch_one(db())
        .await?
        .try_into()?)
    }

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        let entry_id_str = self.entry_id.to_string();
        let list_id_str = self.list_id.to_string();
        let priority_i64 = self.priority as i64;

        sqlx::query!(
            r#"INSERT OR REPLACE INTO entity_list_entries (
                 entry_id, list_id, priority, added_at, entity_slug, note, link
               )
               VALUES (?, ?, ?, ?, ?, ?, ?)"#,
            entry_id_str,
            list_id_str,
            priority_i64,
            self.added_at,
            self.entity_slug,
            self.note,
            self.link,
        )
        .execute(db())
        .await?;

        Ok(())
    }

    #[tracing::instrument]
    async fn delete_from_database(entry_id: Self::DatabaseKey) -> Result<(), Error> {
        let entry_id_str = entry_id.to_string();
        sqlx::query!(
            r#"DELETE FROM entity_list_entries WHERE entry_id = ?"#,
            entry_id_str
        )
        .execute(db())
        .await?;
        Ok(())
    }
}
