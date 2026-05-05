use std::sync::Arc;

use anyhow::{Error, anyhow};
use common::message::dashboard::EntitySummary;
use common::message::user::BasicInfo;
use common::shared::db;
use common::timestring::TimeString;
use poem::{
    IntoResponse, Response, handler,
    session::Session,
    web::{Form, Path, RealIp, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::component::{Component, Head, StandardLayout};
use crate::dashboard::bookmarks::EntityCardModel;
use crate::dashboard::entity_summary;
use crate::entity_list::{EntityList, EntityListEntry, EntityListViewer};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListRole {
    Owner,
    Editor,
    Viewer,
}

impl ListRole {
    fn for_user(list: &EntityList, user_id: Ulid, viewer: Option<&EntityListViewer>) -> Self {
        if list.owner_id == user_id {
            return ListRole::Owner;
        }
        if list.delegate_id == Some(user_id) {
            return ListRole::Editor;
        }
        match viewer {
            Some(v) if v.can_edit => ListRole::Editor,
            _ => ListRole::Viewer,
        }
    }

    fn can_edit(self) -> bool {
        matches!(self, ListRole::Owner | ListRole::Editor)
    }

    fn is_owner(self) -> bool {
        matches!(self, ListRole::Owner)
    }
}

#[handler]
pub async fn page(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;

    let lists = EntityList::get_lists_for_user(user_info.user_id).await?;

    let mut models: Vec<ListModel> = Vec::with_capacity(lists.len());
    for list in lists {
        let entries = EntityListEntry::get_entries(list.list_id).await?;
        let viewers = EntityListViewer::get_for_list(list.list_id).await?;

        let viewer_for_user = viewers.iter().find(|v| v.user_id == user_info.user_id);
        let role = ListRole::for_user(&list, user_info.user_id, viewer_for_user);

        let slugs: Vec<String> = entries.iter().map(|e| e.entity_slug.clone()).collect();
        let summaries = entity_summary::lookup_batch_by_slug(&slugs).await?;

        models.push(ListModel::build(list, role, entries, summaries, viewers).await?);
    }

    render_lists(user_info, models).await
}

#[derive(Debug, Deserialize)]
pub struct CreateListForm {
    pub name: String,
    pub note: String,
    pub private: Option<bool>,
}

#[handler]
pub async fn create(
    _session: &Session,
    Authenticated(user_info): Authenticated,
    Form(form): Form<CreateListForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    if !form.name.trim().is_empty() {
        let list = EntityList {
            list_id: Ulid::new(),
            created_at: TimeString::now().to_string(),
            private: form.private.unwrap_or(false),
            owner_id: user_info.user_id,
            delegate_id: None,
            name: form.name.trim().to_string(),
            note: form.note,
            sidebar: String::new(),
            slug: None,
            hide_identity_form: false,
            image: String::new(),
        };
        list.store().await?;
    }
    Ok(Redirect::see_other("/dashboard/lists").into_response())
}

#[derive(Debug, Deserialize)]
pub struct UpdateListForm {
    pub name: String,
    pub note: String,
    pub private: Option<bool>,
}

#[handler]
pub async fn update(
    _session: &Session,
    Authenticated(user_info): Authenticated,
    Path(list_id): Path<String>,
    Form(form): Form<UpdateListForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let list_ulid = parse_ulid(&list_id, "list id")?;

    let mut list = EntityList::load(list_ulid).await?;
    if list.owner_id != user_info.user_id {
        return Err(anyhow!("Only the owner can edit this list"));
    }

    list.name = form.name;
    list.note = form.note;
    list.private = form.private.unwrap_or(false);
    list.store().await?;

    Ok(Redirect::see_other("/dashboard/lists").into_response())
}

#[handler]
pub async fn delete(
    _session: &Session,
    Authenticated(user_info): Authenticated,
    Path(list_id): Path<String>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let list_ulid = parse_ulid(&list_id, "list id")?;

    let list = EntityList::load(list_ulid).await?;
    if list.owner_id != user_info.user_id {
        return Err(anyhow!("Only the owner can delete this list"));
    }

    let entries = EntityListEntry::get_entries(list_ulid).await?;
    for entry in entries {
        EntityListEntry::delete(entry.entry_id).await?;
    }

    let viewers = EntityListViewer::get_for_list(list_ulid).await?;
    for viewer in viewers {
        EntityListViewer::delete((list_ulid, viewer.user_id)).await?;
    }

    EntityList::delete(list_ulid).await?;

    Ok(Redirect::see_other("/dashboard/lists").into_response())
}

#[derive(Debug, Deserialize)]
pub struct AddEntryForm {
    pub entity_slug: String,
    pub note: String,
}

#[handler]
pub async fn add_entry(
    _session: &Session,
    Authenticated(user_info): Authenticated,
    Path(list_id): Path<String>,
    Form(form): Form<AddEntryForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let list_ulid = parse_ulid(&list_id, "list id")?;

    let role = role_for(&list_ulid, user_info.user_id).await?;
    if !role.can_edit() {
        return Err(anyhow!("You do not have edit access to this list"));
    }

    let slug = form.entity_slug.trim();
    if !slug.is_empty() {
        let exists = sqlx::query_scalar!(
            r#"SELECT slug FROM entities WHERE slug = ? LIMIT 1"#,
            slug
        )
        .fetch_optional(db())
        .await?;

        if exists.is_some() {
            let entry = EntityListEntry {
                entry_id: Ulid::new(),
                list_id: list_ulid,
                priority: 0,
                added_at: TimeString::now().to_string(),
                entity_slug: slug.to_string(),
                note: form.note,
                link: String::new(),
            };
            entry.store().await?;
        }
    }

    Ok(Redirect::see_other("/dashboard/lists").into_response())
}

#[handler]
pub async fn remove_entry(
    _session: &Session,
    Authenticated(user_info): Authenticated,
    Path((list_id, entry_id)): Path<(String, String)>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let list_ulid = parse_ulid(&list_id, "list id")?;
    let entry_ulid = parse_ulid(&entry_id, "entry id")?;

    let role = role_for(&list_ulid, user_info.user_id).await?;
    if !role.can_edit() {
        return Err(anyhow!("You do not have edit access to this list"));
    }

    let entry = EntityListEntry::load(entry_ulid).await?;
    if entry.list_id != list_ulid {
        return Err(anyhow!("Entry does not belong to this list"));
    }

    EntityListEntry::delete(entry_ulid).await?;

    Ok(Redirect::see_other("/dashboard/lists").into_response())
}

#[derive(Debug, Deserialize)]
pub struct SetNoteForm {
    pub note: String,
}

#[handler]
pub async fn set_entry_note(
    _session: &Session,
    Authenticated(user_info): Authenticated,
    Path((list_id, entry_id)): Path<(String, String)>,
    Form(form): Form<SetNoteForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let list_ulid = parse_ulid(&list_id, "list id")?;
    let entry_ulid = parse_ulid(&entry_id, "entry id")?;

    let role = role_for(&list_ulid, user_info.user_id).await?;
    if !role.can_edit() {
        return Err(anyhow!("You do not have edit access to this list"));
    }

    let mut entry = EntityListEntry::load(entry_ulid).await?;
    if entry.list_id != list_ulid {
        return Err(anyhow!("Entry does not belong to this list"));
    }
    entry.note = form.note;
    entry.store().await?;

    Ok(Redirect::see_other("/dashboard/lists").into_response())
}

#[derive(Debug, Deserialize)]
pub struct ShareForm {
    pub viewer_id: String,
    pub can_edit: Option<bool>,
}

#[handler]
pub async fn share(
    _session: &Session,
    Authenticated(user_info): Authenticated,
    Path(list_id): Path<String>,
    Form(form): Form<ShareForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let list_ulid = parse_ulid(&list_id, "list id")?;
    let viewer_ulid = parse_ulid(form.viewer_id.trim(), "viewer id")?;

    let list = EntityList::load(list_ulid).await?;
    if list.owner_id != user_info.user_id {
        return Err(anyhow!("Only the owner can share this list"));
    }
    if viewer_ulid == list.owner_id {
        return Err(anyhow!("The owner is always permitted"));
    }

    let viewer = EntityListViewer {
        list_id: list_ulid,
        user_id: viewer_ulid,
        can_edit: form.can_edit.unwrap_or(false),
        added_at: TimeString::now().to_string(),
    };
    viewer.store().await?;

    Ok(Redirect::see_other("/dashboard/lists").into_response())
}

#[derive(Debug, Deserialize)]
pub struct UnshareForm {
    pub viewer_id: String,
}

#[handler]
pub async fn unshare(
    _session: &Session,
    Authenticated(user_info): Authenticated,
    Path(list_id): Path<String>,
    Form(form): Form<UnshareForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow!("Login required"))?;
    let list_ulid = parse_ulid(&list_id, "list id")?;
    let viewer_ulid = parse_ulid(&form.viewer_id, "viewer id")?;

    let list = EntityList::load(list_ulid).await?;
    if list.owner_id != user_info.user_id {
        return Err(anyhow!("Only the owner can unshare this list"));
    }

    EntityListViewer::delete((list_ulid, viewer_ulid)).await?;

    Ok(Redirect::see_other("/dashboard/lists").into_response())
}

fn parse_ulid(s: &str, label: &str) -> Result<Ulid, Error> {
    s.parse::<Ulid>()
        .map_err(|err| anyhow!("Invalid {}: {}", label, err))
}

async fn role_for(list_id: &Ulid, user_id: Ulid) -> Result<ListRole, Error> {
    let list = EntityList::load(*list_id).await?;
    let viewer = EntityListViewer::load((*list_id, user_id)).await.ok();
    Ok(ListRole::for_user(&list, user_id, viewer.as_ref()))
}

async fn render_lists(
    user_info: Arc<BasicInfo>,
    lists: Vec<ListModel>,
) -> Result<Response, Error> {
    let content = ListsContent { lists };

    let layout = StandardLayout::new("/dashboard/lists", content)
        .prepare(Some(user_info))
        .await?;

    ListsPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

pub struct ListModel {
    pub list_id: Ulid,
    pub name: String,
    pub note: String,
    pub private: bool,
    pub role: ListRole,
    pub entries: Vec<ListEntryModel>,
    pub viewers: Vec<ViewerModel>,
}

impl ListModel {
    async fn build(
        list: EntityList,
        role: ListRole,
        entries: Vec<EntityListEntry>,
        summaries: Vec<EntitySummary>,
        viewers: Vec<EntityListViewer>,
    ) -> Result<Self, Error> {
        let entry_models = entries
            .into_iter()
            .map(|entry| {
                let summary = summaries.iter().find(|s| s.slug == entry.entity_slug);
                ListEntryModel {
                    entry_id: entry.entry_id,
                    entity_slug: entry.entity_slug,
                    note: entry.note,
                    card: summary.map(EntityCardModel::from_summary),
                }
            })
            .collect();

        let mut viewer_models: Vec<ViewerModel> = Vec::with_capacity(viewers.len());
        for viewer in viewers {
            let handle = lookup_handle(viewer.user_id).await;
            viewer_models.push(ViewerModel {
                user_id: viewer.user_id,
                handle,
                can_edit: viewer.can_edit,
            });
        }

        Ok(Self {
            list_id: list.list_id,
            name: list.name,
            note: list.note,
            private: list.private,
            role,
            entries: entry_models,
            viewers: viewer_models,
        })
    }
}

async fn lookup_handle(user_id: Ulid) -> String {
    use common::message::dashboard::PublicProfile;

    let result: Result<PublicProfile, _> =
        common::service::request("user.profile.public_get", user_id.to_string()).await;
    match result
    {
        Ok(profile) if !profile.handle.is_empty() => profile.handle,
        _ => user_id.to_string(),
    }
}

pub struct ListEntryModel {
    pub entry_id: Ulid,
    pub entity_slug: String,
    pub note: String,
    pub card: Option<EntityCardModel>,
}

pub struct ViewerModel {
    pub user_id: Ulid,
    pub handle: String,
    pub can_edit: bool,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/lists.stpl.html")]
struct ListsPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<ListsContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/lists_content.stpl.html")]
struct ListsContent {
    lists: Vec<ListModel>,
}
