use anyhow::Error;
use common::shared::db;
use ulid::Ulid;

use crate::peering::LoadStorePeered;

pub mod page;

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

    /// Lists where the user has a role: owner, delegate, or viewer.
    /// Used for "your lists" on the dashboard. Public lists (private = 0)
    /// are viewable by anyone regardless of role; this method does not
    /// include them unless the user is also a role-holder.
    pub async fn get_lists_for_user(user_id: Ulid) -> Result<Vec<EntityList>, Error> {
        let user_id_str = user_id.to_string();
        let items = sqlx::query_as!(
            EntityListDB,
            r#"SELECT list_id, created_at, private, owner_id, delegate_id, name, note, sidebar, slug, hide_identity_form, image
               FROM entity_lists
               WHERE owner_id = ?
                  OR delegate_id = ?
                  OR list_id IN (SELECT list_id FROM entity_list_viewers WHERE user_id = ?)
               ORDER BY name ASC"#,
            user_id_str,
            user_id_str,
            user_id_str,
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
        // An upsert: REPLACE would delete the row first, and the delete
        // cascades to the list's entries, viewers and identities.
        let list_id_str = self.list_id.to_string();
        let owner_id_str = self.owner_id.to_string();
        let delegate_id_str = self.delegate_id.map(|u| u.to_string());
        let private_i64 = if self.private { 1i64 } else { 0i64 };
        let hide_identity_form_i64 = if self.hide_identity_form { 1i64 } else { 0i64 };

        sqlx::query!(
            r#"INSERT INTO entity_lists (
                 list_id, created_at, private, owner_id, delegate_id, name, note, sidebar, slug, hide_identity_form, image
               )
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT (list_id) DO UPDATE SET
                 created_at = excluded.created_at,
                 private = excluded.private,
                 owner_id = excluded.owner_id,
                 delegate_id = excluded.delegate_id,
                 name = excluded.name,
                 note = excluded.note,
                 sidebar = excluded.sidebar,
                 slug = excluded.slug,
                 hide_identity_form = excluded.hide_identity_form,
                 image = excluded.image"#,
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
    /// Whether the entry is shown on the list's page. An unpublished entry is
    /// a draft placement: it keeps its note, link and position, and simply is
    /// not rendered.
    pub published: bool,
}

struct EntityListEntryDB {
    entry_id: String,
    list_id: String,
    priority: i64,
    added_at: String,
    entity_slug: String,
    note: String,
    link: String,
    published: i64,
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
            published: value.published != 0,
        })
    }
}

impl EntityListEntry {
    pub async fn get_entries(list_id: Ulid) -> Result<Vec<EntityListEntry>, Error> {
        let list_id_str = list_id.to_string();
        let items = sqlx::query_as!(
            EntityListEntryDB,
            r#"SELECT entry_id, list_id, priority, added_at, entity_slug, note, link, published
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

    /// The entries a visitor to the list's page should see.
    ///
    /// Named rather than left to each caller to remember, because the
    /// difference between this and [`get_entries`](Self::get_entries) is the
    /// difference between what the curator is working on and what the world
    /// has been shown.
    pub async fn get_published_entries(list_id: Ulid) -> Result<Vec<EntityListEntry>, Error> {
        Ok(Self::get_entries(list_id)
            .await?
            .into_iter()
            .filter(|entry| entry.published)
            .collect())
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
            r#"SELECT entry_id, list_id, priority, added_at, entity_slug, note, link, published
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
        let published_i64 = i64::from(self.published);

        sqlx::query!(
            r#"INSERT OR REPLACE INTO entity_list_entries (
                 entry_id, list_id, priority, added_at, entity_slug, note, link, published
               )
               VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
            entry_id_str,
            list_id_str,
            priority_i64,
            self.added_at,
            self.entity_slug,
            self.note,
            self.link,
            published_i64,
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

/// A user with view (or edit) access to a private list, in addition to
/// the list's owner and `delegate_id`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EntityListViewer {
    pub list_id: Ulid,
    pub user_id: Ulid,
    pub can_edit: bool,
    pub added_at: String,
}

struct EntityListViewerDB {
    list_id: String,
    user_id: String,
    can_edit: i64,
    added_at: String,
}

impl TryFrom<EntityListViewerDB> for EntityListViewer {
    type Error = anyhow::Error;

    fn try_from(value: EntityListViewerDB) -> Result<Self, Self::Error> {
        Ok(EntityListViewer {
            list_id: value.list_id.parse()?,
            user_id: value.user_id.parse()?,
            can_edit: value.can_edit != 0,
            added_at: value.added_at,
        })
    }
}

impl EntityListViewer {
    pub async fn get_for_list(list_id: Ulid) -> Result<Vec<EntityListViewer>, Error> {
        let list_id_str = list_id.to_string();
        let items = sqlx::query_as!(
            EntityListViewerDB,
            r#"SELECT list_id, user_id, can_edit, added_at
               FROM entity_list_viewers
               WHERE list_id = ?
               ORDER BY added_at ASC"#,
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

impl LoadStorePeered for EntityListViewer {
    type DatabaseKey = (Ulid, Ulid);

    const GROUP: &str = "entity-list-viewer";

    #[tracing::instrument]
    async fn read_from_database(key: Self::DatabaseKey) -> Result<Self, Error> {
        let (list_id, user_id) = key;
        let list_id_str = list_id.to_string();
        let user_id_str = user_id.to_string();
        Ok(sqlx::query_as!(
            EntityListViewerDB,
            r#"SELECT list_id, user_id, can_edit, added_at
               FROM entity_list_viewers
               WHERE list_id = ? AND user_id = ?"#,
            list_id_str,
            user_id_str,
        )
        .fetch_one(db())
        .await?
        .try_into()?)
    }

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        let list_id_str = self.list_id.to_string();
        let user_id_str = self.user_id.to_string();
        let can_edit_i64 = if self.can_edit { 1i64 } else { 0i64 };

        sqlx::query!(
            r#"INSERT OR REPLACE INTO entity_list_viewers (
                 list_id, user_id, can_edit, added_at
               )
               VALUES (?, ?, ?, ?)"#,
            list_id_str,
            user_id_str,
            can_edit_i64,
            self.added_at,
        )
        .execute(db())
        .await?;

        Ok(())
    }

    #[tracing::instrument]
    async fn delete_from_database(key: Self::DatabaseKey) -> Result<(), Error> {
        let (list_id, user_id) = key;
        let list_id_str = list_id.to_string();
        let user_id_str = user_id.to_string();
        sqlx::query!(
            r#"DELETE FROM entity_list_viewers WHERE list_id = ? AND user_id = ?"#,
            list_id_str,
            user_id_str,
        )
        .execute(db())
        .await?;
        Ok(())
    }
}

/// The name a participant gave when the list asked who they are.
///
/// Separate from [`EntityListViewer`], which records who the owner shared
/// the list with. Naming yourself is not a grant of access, and a
/// participant who does so must not appear in the owner's share list.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EntityListIdentity {
    pub list_id: Ulid,
    pub user_id: Ulid,
    pub first_name: String,
    pub last_name: String,
    pub given_at: String,
}

struct EntityListIdentityDB {
    list_id: String,
    user_id: String,
    first_name: String,
    last_name: String,
    given_at: String,
}

impl TryFrom<EntityListIdentityDB> for EntityListIdentity {
    type Error = anyhow::Error;

    fn try_from(value: EntityListIdentityDB) -> Result<Self, Self::Error> {
        Ok(EntityListIdentity {
            list_id: value.list_id.parse()?,
            user_id: value.user_id.parse()?,
            first_name: value.first_name,
            last_name: value.last_name,
            given_at: value.given_at,
        })
    }
}

impl EntityListIdentity {
    pub async fn get_for_list(list_id: Ulid) -> Result<Vec<EntityListIdentity>, Error> {
        let list_id_str = list_id.to_string();
        let items = sqlx::query_as!(
            EntityListIdentityDB,
            r#"SELECT list_id, user_id, first_name, last_name, given_at
               FROM entity_list_identities
               WHERE list_id = ?"#,
            list_id_str,
        )
        .fetch_all(db())
        .await?;

        items
            .into_iter()
            .map(|row| row.try_into())
            .collect::<Result<Vec<_>, _>>()
    }

    /// The participant's name as one string, empty when they gave neither
    /// half. Callers fall back to the account handle.
    pub fn display_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
            .trim()
            .to_string()
    }
}

impl LoadStorePeered for EntityListIdentity {
    type DatabaseKey = (Ulid, Ulid);

    const GROUP: &str = "entity-list-identity";

    #[tracing::instrument]
    async fn read_from_database(key: Self::DatabaseKey) -> Result<Self, Error> {
        let (list_id, user_id) = key;
        let list_id_str = list_id.to_string();
        let user_id_str = user_id.to_string();
        Ok(sqlx::query_as!(
            EntityListIdentityDB,
            r#"SELECT list_id, user_id, first_name, last_name, given_at
               FROM entity_list_identities
               WHERE list_id = ? AND user_id = ?"#,
            list_id_str,
            user_id_str,
        )
        .fetch_one(db())
        .await?
        .try_into()?)
    }

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        let list_id_str = self.list_id.to_string();
        let user_id_str = self.user_id.to_string();

        sqlx::query!(
            r#"INSERT OR REPLACE INTO entity_list_identities (
                 list_id, user_id, first_name, last_name, given_at
               )
               VALUES (?, ?, ?, ?, ?)"#,
            list_id_str,
            user_id_str,
            self.first_name,
            self.last_name,
            self.given_at,
        )
        .execute(db())
        .await?;

        Ok(())
    }

    #[tracing::instrument]
    async fn delete_from_database(key: Self::DatabaseKey) -> Result<(), Error> {
        let (list_id, user_id) = key;
        let list_id_str = list_id.to_string();
        let user_id_str = user_id.to_string();
        sqlx::query!(
            r#"DELETE FROM entity_list_identities WHERE list_id = ? AND user_id = ?"#,
            list_id_str,
            user_id_str,
        )
        .execute(db())
        .await?;
        Ok(())
    }
}
