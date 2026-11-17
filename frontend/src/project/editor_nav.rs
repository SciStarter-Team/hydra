//! The bar of links to a project's editor-only pages, as orrery's project
//! submenu laid them out. The bar is shown only to those who may edit the
//! project (`edit_access::may_edit`). The project's own pages it links to --
//! its participants, messages and stats -- are open to exactly them; the
//! People Finder and the Tools page are not the project's, and are open more
//! widely.

use anyhow::Error;
use common::message::user::BasicInfo;
use poem::{IntoResponse, Response, http::StatusCode, web::Redirect};
use sailfish::TemplateOnce;
use std::sync::Arc;
use ulid::Ulid;

use crate::{
    dashboard::entity_messages,
    edit_access::{self, Editor, Page},
    entity::{Entity, EntityKind},
    peering::LoadStorePeered,
    people_finder,
};

/// Whether a request may see one of a project's editor-only pages.
pub enum Gate {
    /// It may: it comes from `user`, who may edit the project `project_id`.
    Open {
        user: Arc<BasicInfo>,
        project_id: Ulid,
    },
    /// It may not, and is answered with this: a visitor who is not signed
    /// in is sent to sign in and come back to `here`, and anyone else is
    /// told there is no such page, rather than that it is closed to them.
    Closed(Response),
}

/// Whether the request for `here`, a page of the project whose slug is
/// `slug`, from `user_info`, may see it.
pub async fn gate(
    slug: &str,
    here: &str,
    user_info: Option<Arc<BasicInfo>>,
) -> Result<Gate, Error> {
    let Some(user) = user_info else {
        let next = format!("/login?next={}", urlencoding::encode(here));
        return Ok(Gate::Closed(Redirect::see_other(next).into_response()));
    };

    let Some(project_id) = project_of(slug).await? else {
        return Ok(Gate::Closed(StatusCode::NOT_FOUND.into_response()));
    };

    if !edit_access::may_edit(Page::Project(project_id), Editor::from(&*user)).await? {
        return Ok(Gate::Closed(StatusCode::NOT_FOUND.into_response()));
    }

    Ok(Gate::Open { user, project_id })
}

/// The project a slug names, if it names one.
async fn project_of(slug: &str) -> Result<Option<Ulid>, Error> {
    match Entity::load(slug.to_string()).await {
        Ok(entity) if matches!(entity.kind(), EntityKind::Project) => Ok(entity.target().as_ulid()),
        Ok(_) => Ok(None),
        Err(err) => match err.downcast_ref::<sqlx::Error>() {
            Some(sqlx::Error::RowNotFound) => Ok(None),
            _ => Err(err),
        },
    }
}

/// The Tools page: a content page (the `project-tools` entity), the same for
/// every project, saying what each of the others is for.
pub const TOOLS_PATH: &str = "/project-tools";

/// One of the pages the bar links to, in the order orrery's bar had them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorPage {
    Project,
    Recruitment,
    Participants,
    Messages,
    Stats,
    Tools,
}

impl EditorPage {
    const ALL: [EditorPage; 6] = [
        EditorPage::Project,
        EditorPage::Recruitment,
        EditorPage::Participants,
        EditorPage::Messages,
        EditorPage::Stats,
        EditorPage::Tools,
    ];

    fn label(self) -> &'static str {
        match self {
            EditorPage::Project => "Project",
            EditorPage::Recruitment => "Recruitment",
            EditorPage::Participants => "Volunteer Manager",
            EditorPage::Messages => "Messages",
            EditorPage::Stats => "Stats",
            EditorPage::Tools => "Tools",
        }
    }
}

/// The bar, on the page `current` of the project `project_id`, whose slug is
/// `slug`.
#[derive(TemplateOnce)]
#[template(path = "component/project_editor_nav.stpl.html")]
pub struct EditorNav {
    links: Vec<Link>,
}

struct Link {
    label: &'static str,
    href: String,
    current: bool,
}

impl EditorNav {
    pub fn new(slug: &str, project_id: impl std::fmt::Display, current: EditorPage) -> EditorNav {
        let page = EntityKind::Project.path(slug);

        EditorNav {
            links: EditorPage::ALL
                .into_iter()
                .map(|target| Link {
                    label: target.label(),
                    href: match target {
                        EditorPage::Project => page.clone(),
                        EditorPage::Messages => entity_messages::inbox_path(&project_id),
                        EditorPage::Participants => super::participants::path_from_page(&page),
                        EditorPage::Stats => super::stats::path_from_page(&page),
                        EditorPage::Recruitment => people_finder::PATH.to_string(),
                        EditorPage::Tools => TOOLS_PATH.to_string(),
                    },
                    current: target == current,
                })
                .collect(),
        }
    }
}
