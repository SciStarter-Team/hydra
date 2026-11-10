//! Admin management of the Zooniverse projects whose classifications are
//! credited to SciStarter projects, and which of SciStarter's Zooniverse
//! accounts each is read as.
//!
//! The connections are log-service's, whose bridge reads the
//! classifications. While orrery is the primary, most are kept in orrery's
//! admin and copied here, so those are listed but not changed here; a
//! connection made here can be. Connecting a project, or moving it to the
//! other account, needs Panoptes to confirm that the account can read it,
//! and the page can ask Panoptes about any project before then.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::admin::autocomplete::{EntityAutocomplete, resolve_project_name, resolve_project_names};
use crate::component::{AdminLayout, Component};
use crate::middleware::authorization::Authenticated;
use common::message::log::{
    ZooniverseAccess, ZooniverseAccessRequest, ZooniverseProject, ZooniverseProjectAdd,
    ZooniverseProjectAdded, ZooniverseProjectDelete, ZooniverseProjectUpdate,
    ZooniverseProjectUpdated, ZooniverseReader,
};
use common::message::user::BasicInfo;
use common::service::{request, request_timeout};
use poem::{
    Endpoint, Route, get, handler, post,
    web::{Form, Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

/// How long to wait for an answer that needs Panoptes asked: log-service
/// bounds its check at 40s, and htmx gives up at 60s.
const PANOPTES_WAIT: Duration = Duration::from_secs(50);

// ==================== Templates ====================

#[derive(TemplateOnce)]
#[template(path = "admin/zooniverse_projects.stpl.html")]
struct ZooniverseProjectsPage {
    rows: Vec<ZooniverseProjectRow>,

    /// Whether any project listed is kept in orrery's admin.
    any_from_orrery: bool,

    /// What the add form holds: empty, or what was asked for and refused.
    form: AddFormState,

    /// The connection just made.
    added: Option<ZooniverseProjectRow>,

    /// Why the connection asked for was not made.
    refusal: Option<Refusal>,
}

impl ZooniverseProjectsPage {
    fn new(
        rows: Vec<ZooniverseProjectRow>,
        form: AddFormState,
        added: Option<ZooniverseProjectRow>,
        refusal: Option<Refusal>,
    ) -> ZooniverseProjectsPage {
        ZooniverseProjectsPage {
            any_from_orrery: rows.iter().any(|row| row.project.orrery_owns),
            rows,
            form,
            added,
            refusal,
        }
    }
}

struct AddFormState {
    project: EntityAutocomplete,
    zooniverse_id: String,
    reader: ZooniverseReader,
}

impl AddFormState {
    fn empty() -> AddFormState {
        AddFormState {
            project: project_selector(),
            zooniverse_id: String::new(),
            reader: ZooniverseReader::DEFAULT,
        }
    }
}

fn project_selector() -> EntityAutocomplete {
    EntityAutocomplete::new_project_selector(
        "entity_id",
        "SciStarter project",
        "Search for a project...",
    )
}

/// The project selector, holding `entity_id` if one was chosen, under its
/// name if it is known here.
async fn project_selector_holding(
    entity_id: Option<Ulid>,
) -> Result<EntityAutocomplete, anyhow::Error> {
    let Some(entity_id) = entity_id else {
        return Ok(project_selector());
    };

    let name = resolve_project_name(entity_id)
        .await?
        .unwrap_or_else(|| entity_id.to_string());

    Ok(project_selector().with_selected(entity_id, name))
}

/// Why a change was refused, with what Panoptes said if that is why.
#[derive(Clone)]
struct Refusal {
    message: String,
    access: Option<ZooniverseAccessReport>,
}

/// The choice of which account a project is read as.
#[derive(TemplateOnce)]
#[template(path = "admin/partials/zooniverse_reader_select.stpl.html")]
pub struct ReaderSelect<'a> {
    pub id: &'a str,
    pub selected: ZooniverseReader,
}

/// One connected project, as the list shows it.
#[derive(Clone, TemplateOnce)]
#[template(path = "admin/partials/zooniverse_project_row.stpl.html")]
pub struct ZooniverseProjectRow {
    project: ZooniverseProject,

    /// The SciStarter project's name, if it is known here.
    name: Option<String>,

    /// Why the last change asked of it was refused.
    refusal: Option<Refusal>,
}

/// One connected project, as it is edited in place.
#[derive(TemplateOnce)]
#[template(path = "admin/partials/zooniverse_project_edit.stpl.html")]
struct ZooniverseProjectEdit {
    project: ZooniverseProject,
    selector: EntityAutocomplete,
    reader: ZooniverseReader,
    refusal: Option<Refusal>,
}

/// What Panoptes says about reading a project as each reader.
#[derive(Clone, TemplateOnce)]
#[template(path = "admin/partials/zooniverse_access.stpl.html")]
pub struct ZooniverseAccessReport {
    zooniverse_id: i64,
    accesses: Vec<ZooniverseAccess>,
}

/// What Panoptes said about reading a project as one reader, in a line, as
/// orrery's `check_zooniverse_access` puts it.
pub fn access_summary(access: &ZooniverseAccess) -> String {
    if let Some(trouble) = &access.trouble {
        trouble.clone()
    } else if access.granted() {
        format!(
            "can read its classifications{}",
            if access.readable {
                ""
            } else {
                ", though there are none yet"
            }
        )
    } else {
        "not a collaborator, so its classifications would read as empty".to_string()
    }
}

/// The account a reader acts as, as Panoptes named it.
pub fn access_account(access: &ZooniverseAccess) -> Option<String> {
    access
        .account
        .as_ref()
        .map(|account| format!("@{}, Panoptes id {}", account.login, account.id))
}

/// The refusal for a reader Panoptes didn't confirm can read the project.
fn not_granted(zooniverse_id: i64, access: ZooniverseAccess) -> Refusal {
    Refusal {
        message: format!(
            "Panoptes didn't confirm that {} can read Panoptes project {zooniverse_id}. Check \
             access to see whether the project's team made the other account a collaborator.",
            access.reader.label()
        ),
        access: Some(ZooniverseAccessReport {
            zooniverse_id,
            accesses: vec![access],
        }),
    }
}

// ==================== Forms ====================

#[derive(Debug, Deserialize)]
struct AddForm {
    #[serde(default)]
    entity_id: String,

    #[serde(default)]
    zooniverse_id: String,

    reader: Option<ZooniverseReader>,
}

#[derive(Debug, Deserialize)]
struct EditForm {
    #[serde(default)]
    entity_id: String,

    reader: ZooniverseReader,
}

#[derive(Debug, Deserialize)]
struct AccessQuery {
    #[serde(default)]
    zooniverse_id: String,
}

/// A Panoptes id as typed, if it is one.
fn parse_zooniverse_id(typed: &str) -> Option<i64> {
    typed.trim().parse().ok().filter(|id| *id > 0)
}

/// A SciStarter project as the selector holds it, if one was chosen.
fn parse_entity_id(held: &str) -> Option<Ulid> {
    held.trim().parse().ok()
}

// ==================== Handlers ====================

async fn rows(
    projects: Vec<ZooniverseProject>,
) -> Result<Vec<ZooniverseProjectRow>, anyhow::Error> {
    let names: HashMap<Ulid, String> =
        resolve_project_names(projects.iter().map(|p| p.entity_id).collect())
            .await?
            .into_iter()
            .collect();

    let mut rows: Vec<ZooniverseProjectRow> = projects
        .into_iter()
        .map(|project| ZooniverseProjectRow {
            name: names.get(&project.entity_id).cloned(),
            project,
            refusal: None,
        })
        .collect();

    rows.sort_by(|a, b| {
        (a.name.is_none(), &a.name, a.project.zooniverse_id).cmp(&(
            b.name.is_none(),
            &b.name,
            b.project.zooniverse_id,
        ))
    });

    Ok(rows)
}

async fn row(project: ZooniverseProject) -> Result<ZooniverseProjectRow, anyhow::Error> {
    Ok(rows(vec![project]).await?.remove(0))
}

async fn render_page(
    user_info: Option<Arc<BasicInfo>>,
    page: ZooniverseProjectsPage,
) -> Result<poem::Response, anyhow::Error> {
    let prepared = AdminLayout::new("Zooniverse Projects", page)
        .with_sidebar_active("engine-room-zooniverse-projects")
        .with_breadcrumb("Engine Room", None)
        .with_breadcrumb("Zooniverse Projects", None)
        .prepare(user_info)
        .await?;

    Ok(html(prepared.render_once()?))
}

fn html(body: String) -> poem::Response {
    poem::Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(body)
}

async fn listed() -> Result<Vec<ZooniverseProjectRow>, anyhow::Error> {
    rows(request("log.zooniverse.admin.list", ()).await?).await
}

#[handler]
async fn projects_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let page = ZooniverseProjectsPage::new(listed().await?, AddFormState::empty(), None, None);

    render_page(user_info, page).await
}

#[handler]
async fn project_create(
    Form(form): Form<AddForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let reader = form.reader.unwrap_or(ZooniverseReader::DEFAULT);
    let entity_id = parse_entity_id(&form.entity_id);
    let zooniverse_id = parse_zooniverse_id(&form.zooniverse_id);

    let outcome = match (entity_id, zooniverse_id) {
        (None, _) => Err(Refusal {
            message: "Choose the SciStarter project to credit.".to_string(),
            access: None,
        }),
        (_, None) => Err(Refusal {
            message: "The Panoptes project id is the number on the project's Zooniverse \
                      page, such as 30240."
                .to_string(),
            access: None,
        }),
        (Some(entity_id), Some(zooniverse_id)) => {
            let added: ZooniverseProjectAdded = request_timeout(
                "log.zooniverse.admin.add",
                ZooniverseProjectAdd {
                    entity_id,
                    zooniverse_id,
                    reader,
                },
                PANOPTES_WAIT,
            )
            .await?;

            match added {
                ZooniverseProjectAdded::Added(project) => Ok(project),
                ZooniverseProjectAdded::AlreadyConnected(existing) => {
                    let existing = row(existing).await?;
                    Err(Refusal {
                        message: format!(
                            "Panoptes project {zooniverse_id} is connected already, to {}.",
                            existing.name.as_deref().unwrap_or("a project unknown here")
                        ),
                        access: None,
                    })
                }
                ZooniverseProjectAdded::InvalidProjectId => Err(Refusal {
                    message: "A Panoptes project id is a positive number.".to_string(),
                    access: None,
                }),
                ZooniverseProjectAdded::NotGranted(access) => {
                    Err(not_granted(zooniverse_id, access))
                }
            }
        }
    };

    let (added, refusal) = match outcome {
        Ok(project) => {
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "zooniverse-project-create",
                    user_info.user_id,
                    Some(project.entity_id),
                    Some(format!(
                        "{} {}",
                        project.zooniverse_id,
                        project.reader.as_str()
                    )),
                )
                .await?;
            }

            (Some(row(project).await?), None)
        }
        Err(refusal) => (None, Some(refusal)),
    };

    let form = if refusal.is_some() {
        AddFormState {
            project: project_selector_holding(entity_id).await?,
            zooniverse_id: form.zooniverse_id,
            reader,
        }
    } else {
        AddFormState::empty()
    };

    let page = ZooniverseProjectsPage::new(listed().await?, form, added, refusal);

    render_page(user_info, page).await
}

/// What Panoptes says about reading a project as each reader, for the add
/// form or a listed project.
#[handler]
async fn project_access(Query(query): Query<AccessQuery>) -> Result<poem::Response, anyhow::Error> {
    let Some(zooniverse_id) = parse_zooniverse_id(&query.zooniverse_id) else {
        return Ok(html(
            r#"<p class="text-sm text-red-700">Enter the Panoptes project id first.</p>"#
                .to_string(),
        ));
    };

    let accesses: Vec<ZooniverseAccess> = request_timeout(
        "log.zooniverse.admin.access",
        ZooniverseAccessRequest { zooniverse_id },
        PANOPTES_WAIT,
    )
    .await?;

    Ok(html(
        ZooniverseAccessReport {
            zooniverse_id,
            accesses,
        }
        .render_once()?,
    ))
}

async fn connected(zooniverse_id: i64) -> Result<Option<ZooniverseProject>, anyhow::Error> {
    let projects: Vec<ZooniverseProject> = request("log.zooniverse.admin.list", ()).await?;

    Ok(projects
        .into_iter()
        .find(|project| project.zooniverse_id == zooniverse_id))
}

/// A listed project's row, as it is shown, which cancelling an edit returns
/// to.
#[handler]
async fn project_row(Path(zooniverse_id): Path<i64>) -> Result<poem::Response, anyhow::Error> {
    let Some(project) = connected(zooniverse_id).await? else {
        return Ok(html(String::new()));
    };

    Ok(html(row(project).await?.render_once()?))
}

#[handler]
async fn project_edit(Path(zooniverse_id): Path<i64>) -> Result<poem::Response, anyhow::Error> {
    let Some(project) = connected(zooniverse_id).await? else {
        return Ok(html(String::new()));
    };

    if project.orrery_owns {
        return Ok(html(row(project).await?.render_once()?));
    }

    Ok(html(
        ZooniverseProjectEdit {
            selector: project_selector_holding(Some(project.entity_id)).await?,
            reader: project.reader,
            project,
            refusal: None,
        }
        .render_once()?,
    ))
}

#[handler]
async fn project_update(
    Path(zooniverse_id): Path<i64>,
    Form(form): Form<EditForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let Some(current) = connected(zooniverse_id).await? else {
        return Ok(html(String::new()));
    };

    let refused =
        async |refusal: Refusal, entity_id: Ulid| -> Result<poem::Response, anyhow::Error> {
            Ok(html(
                ZooniverseProjectEdit {
                    selector: project_selector_holding(Some(entity_id)).await?,
                    reader: form.reader,
                    project: current.clone(),
                    refusal: Some(refusal),
                }
                .render_once()?,
            ))
        };

    let Some(entity_id) = parse_entity_id(&form.entity_id) else {
        let refusal = Refusal {
            message: "Choose the SciStarter project to credit.".to_string(),
            access: None,
        };
        return refused(refusal, current.entity_id).await;
    };

    let updated: ZooniverseProjectUpdated = request_timeout(
        "log.zooniverse.admin.update",
        ZooniverseProjectUpdate {
            zooniverse_id,
            entity_id,
            reader: form.reader,
        },
        PANOPTES_WAIT,
    )
    .await?;

    match updated {
        ZooniverseProjectUpdated::Updated(project) => {
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "zooniverse-project-update",
                    user_info.user_id,
                    Some(project.entity_id),
                    Some(format!(
                        "{} {}",
                        project.zooniverse_id,
                        project.reader.as_str()
                    )),
                )
                .await?;
            }

            Ok(html(row(project).await?.render_once()?))
        }
        ZooniverseProjectUpdated::NotFound => Ok(html(String::new())),
        ZooniverseProjectUpdated::OrreryOwns(project) => {
            let mut row = row(project).await?;
            row.refusal = Some(Refusal {
                message: "This project is now connected in orrery's admin, so it is changed there."
                    .to_string(),
                access: None,
            });
            Ok(html(row.render_once()?))
        }
        ZooniverseProjectUpdated::NotGranted(access) => {
            refused(not_granted(zooniverse_id, access), entity_id).await
        }
    }
}

#[handler]
async fn project_delete(
    Path(zooniverse_id): Path<i64>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let () = request(
        "log.zooniverse.admin.delete",
        ZooniverseProjectDelete { zooniverse_id },
    )
    .await?;

    if let Some(user_info) = &user_info {
        common::service::log_activity(
            None,
            "zooniverse-project-delete",
            user_info.user_id,
            None,
            Some(zooniverse_id.to_string()),
        )
        .await?;
    }

    // A project orrery keeps is still listed, and stays.
    match connected(zooniverse_id).await? {
        Some(project) => Ok(html(row(project).await?.render_once()?)),
        None => Ok(html(String::new())),
    }
}

// ==================== Routes ====================

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(projects_list))
        .at("/create", post(project_create))
        .at("/access", get(project_access))
        .at(
            "/:zooniverse_id",
            get(project_row).put(project_update).delete(project_delete),
        )
        .at("/:zooniverse_id/edit", get(project_edit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;
    use common::message::user::ZooniverseAccount;

    fn project(zooniverse_id: i64, orrery_owns: bool) -> ZooniverseProject {
        ZooniverseProject {
            zooniverse_id,
            entity_id: "01JKXMG5WNN9CJJ6VAWF62645Z".parse().unwrap(),
            reader: ZooniverseReader::Legacy,
            orrery_owns,
            last_classification_id: 790276182,
        }
    }

    fn row(zooniverse_id: i64, orrery_owns: bool) -> ZooniverseProjectRow {
        ZooniverseProjectRow {
            project: project(zooniverse_id, orrery_owns),
            name: Some("Galaxy <Zoo>".to_string()),
            refusal: None,
        }
    }

    fn access(reader: ZooniverseReader, roles: &[&str], readable: bool) -> ZooniverseAccess {
        ZooniverseAccess {
            reader,
            account: Some(ZooniverseAccount {
                id: 3128908,
                login: "scistarter".to_string(),
            }),
            roles: roles.iter().map(|role| role.to_string()).collect(),
            readable,
            trouble: None,
        }
    }

    #[test]
    fn only_a_connection_made_here_offers_to_be_changed() {
        let html = ZooniverseProjectsPage::new(
            vec![row(6250, true), row(30240, false)],
            AddFormState::empty(),
            None,
            None,
        )
        .render_once()
        .unwrap();

        check!(html.contains("Galaxy &lt;Zoo&gt;"));
        check!(!html.contains("Galaxy <Zoo>"));
        check!(html.contains(r#"hx-delete="/-admin/zooniverse-projects/30240""#));
        check!(!html.contains(r#"hx-delete="/-admin/zooniverse-projects/6250""#));
        check!(html.contains(r#"hx-get="/-admin/zooniverse-projects/30240/edit""#));
        check!(!html.contains(r#"hx-get="/-admin/zooniverse-projects/6250/edit""#));
        check!(html.contains("from orrery"));
        check!(html.contains("790276182"));
        check!(!html.contains(r#"role="alert""#));
    }

    #[test]
    fn the_add_form_reads_new_projects_as_scistarters_account() {
        let html = ZooniverseProjectsPage::new(vec![], AddFormState::empty(), None, None)
            .render_once()
            .unwrap();

        check!(html.contains(r#"<option value="scistarter" selected>"#));
        check!(html.contains(r#"<option value="legacy">"#));
        check!(!html.contains("from orrery"));
    }

    #[test]
    fn a_refusal_keeps_what_was_typed_and_says_what_panoptes_said() {
        let html = ZooniverseProjectsPage::new(
            vec![],
            AddFormState {
                project: project_selector(),
                zooniverse_id: "30240\"".to_string(),
                reader: ZooniverseReader::Legacy,
            },
            None,
            Some(not_granted(
                30240,
                access(ZooniverseReader::Legacy, &["tester"], false),
            )),
        )
        .render_once()
        .unwrap();

        check!(html.contains(r#"role="alert""#));
        check!(html.contains(r#"value="30240&quot;""#));
        check!(html.contains(r#"<option value="legacy" selected>"#));
        check!(html.contains("@scistarter, Panoptes id 3128908"));
        check!(html.contains("not a collaborator"));
    }

    #[test]
    fn the_access_report_says_which_reader_to_connect_with() {
        let html = ZooniverseAccessReport {
            zooniverse_id: 30240,
            accesses: vec![
                ZooniverseAccess {
                    trouble: Some("Panoptes would not issue a token".to_string()),
                    account: None,
                    ..access(ZooniverseReader::Legacy, &[], false)
                },
                access(ZooniverseReader::Scistarter, &["collaborator"], false),
            ],
        }
        .render_once()
        .unwrap();

        check!(html.contains("Panoptes would not issue a token"));
        check!(html.contains("can read its classifications, though there are none yet"));
        check!(html.contains("collaborator"));
    }

    #[test]
    fn a_panoptes_id_is_a_positive_number() {
        check!(parse_zooniverse_id(" 30240 ") == Some(30240));
        check!(parse_zooniverse_id("0").is_none());
        check!(parse_zooniverse_id("-6250").is_none());
        check!(parse_zooniverse_id("galaxy-zoo").is_none());
    }
}
