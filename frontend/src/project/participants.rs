//! `/projects/<slug>/participants`: the Volunteer Manager, for the people
//! who may edit a project and for SciStarter's staff, as orrery had it. It lists everyone who has joined or saved the project, lets
//! its managers star people and filter on the stars, and write to whichever
//! of them they pick, as the project.
//!
//! The roster and its stars come from the user service, contributions from
//! the log service's daily snapshot, and when the project last wrote to each
//! person from the forum service. See `common::message::participants`.

use anyhow::{Error, anyhow};
use common::{
    message::{
        forum::{SendBulkMessage, SendBulkResponse},
        participants::{
            ContributionsRequest, LastContacted, LastContactedRequest, Participant,
            ParticipantContributions, ParticipantKind, RosterMembersRequest, RosterRequest,
            RosterResponse, StarRequest,
        },
        user::BasicInfo,
    },
    service::request,
    shared::db,
};
use jiff::Timestamp;
use poem::{
    IntoResponse, Response, handler,
    http::StatusCode,
    web::{Html, Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};
use ulid::Ulid;

use crate::{
    component::{Component, Head, StandardLayout},
    entity::EntityKind,
    form::{QsForm, deserialize_string_or_vec},
    i18n::format_date,
    middleware::authorization::Authenticated,
    number::thousands,
    project::editor_nav::{EditorNav, EditorPage, Gate, gate},
};

/// How many people a page of the roster lists.
const PAGE_SIZE: u32 = 50;

/// The path of a project's Volunteer Manager.
pub fn path(slug: &str) -> String {
    path_from_page(&EntityKind::Project.path(slug))
}

/// The path of the Volunteer Manager of the project whose page is at
/// `project_page`.
pub fn path_from_page(project_page: &str) -> String {
    format!("{project_page}/participants")
}

/// The path that stars, or with DELETE unstars, someone on the roster.
fn star_path(slug: &str, kind: ParticipantKind, user_id: Ulid) -> String {
    format!("{}/{}/{user_id}/star", path(slug), kind.key())
}

/// The path that writes to the people picked from the roster.
fn message_path(slug: &str) -> String {
    format!("{}/message", path(slug))
}

/// The id of the cell saying when the project last wrote to someone, which
/// a message sent from the page brings up to date.
fn contacted_id(user_id: Ulid) -> String {
    format!("pm-contacted-{user_id}")
}

/// Which of the starred the roster shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StarFilter {
    Starred,
    Unstarred,
}

impl StarFilter {
    const ALL: [StarFilter; 2] = [StarFilter::Starred, StarFilter::Unstarred];

    fn key(self) -> &'static str {
        match self {
            StarFilter::Starred => "starred",
            StarFilter::Unstarred => "unstarred",
        }
    }

    fn from_key(key: &str) -> Option<StarFilter> {
        StarFilter::ALL
            .into_iter()
            .find(|filter| filter.key() == key)
    }

    fn label(self) -> &'static str {
        match self {
            StarFilter::Starred => "Starred",
            StarFilter::Unstarred => "Not starred",
        }
    }

    fn starred(self) -> bool {
        self == StarFilter::Starred
    }
}

/// The roster's filters and page, as the page's address carries them.
#[derive(Debug, Default, Deserialize)]
pub struct RosterQuery {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    starred: String,
    #[serde(default)]
    q: String,
    #[serde(default)]
    page: Option<u32>,
}

/// The filters a [`RosterQuery`] names, with whatever it names that is not
/// a filter left out.
struct Filters {
    kind: Option<ParticipantKind>,
    starred: Option<StarFilter>,
    search: String,
    page: u32,
}

impl Filters {
    fn of(query: RosterQuery) -> Filters {
        Filters {
            kind: ParticipantKind::from_key(&query.kind),
            starred: StarFilter::from_key(&query.starred),
            search: query.q.trim().to_string(),
            page: query.page.unwrap_or(1).max(1),
        }
    }

    /// The roster at page `number` under these filters.
    fn href(&self, here: &str, number: u32) -> String {
        let mut pairs = Vec::new();
        if let Some(kind) = self.kind {
            pairs.push(format!("kind={}", kind.key()));
        }
        if let Some(starred) = self.starred {
            pairs.push(format!("starred={}", starred.key()));
        }
        if !self.search.is_empty() {
            pairs.push(format!("q={}", urlencoding::encode(&self.search)));
        }
        if number > 1 {
            pairs.push(format!("page={number}"));
        }

        if pairs.is_empty() {
            here.to_string()
        } else {
            format!("{here}?{}", pairs.join("&"))
        }
    }
}

/// What the page needs to know of the project itself.
struct ProjectFacts {
    name: String,
    is_affiliate: bool,
    /// The project asks its participants to agree to a consent statement.
    asks_consent: bool,
}

async fn project_facts(project_id: Ulid) -> Result<Option<ProjectFacts>, Error> {
    let row = sqlx::query!(
        r#"SELECT "name", "is_affiliate" AS "is_affiliate: bool",
                  "consent_statement" <> '' AS "asks_consent!: bool"
           FROM "projects" WHERE "project_id" = ?"#,
        project_id.to_string(),
    )
    .fetch_optional(db())
    .await?;

    Ok(row.map(|row| ProjectFacts {
        name: row.name,
        is_affiliate: row.is_affiliate,
        asks_consent: row.asks_consent,
    }))
}

#[handler]
pub async fn page(
    Path(slug): Path<String>,
    Query(query): Query<RosterQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let here = path(&slug);

    let (user, project_id) = match gate(&slug, &here, user_info).await? {
        Gate::Open { user, project_id } => (user, project_id),
        Gate::Closed(response) => return Ok(response),
    };

    let Some(project) = project_facts(project_id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let filters = Filters::of(query);
    let roster: RosterResponse = request(
        "user.roster.list",
        RosterRequest {
            project_id,
            kind: filters.kind,
            starred: filters.starred.map(StarFilter::starred),
            search: filters.search.clone(),
            offset: (filters.page - 1) * PAGE_SIZE,
            limit: PAGE_SIZE,
        },
    )
    .await?;

    let rows = rows(&slug, project_id, roster.participants).await?;
    let pages = roster.total.div_ceil(PAGE_SIZE).max(1);

    let content = ParticipantsContent {
        nav: EditorNav::new(&slug, project_id, EditorPage::Participants),
        project_path: EntityKind::Project.path(&slug),
        name: project.name.clone(),
        is_affiliate: project.is_affiliate,
        asks_consent: project.asks_consent,
        kinds: ParticipantKind::ALL
            .into_iter()
            .map(|kind| Choice {
                key: kind.key(),
                label: kind.label(),
                selected: filters.kind == Some(kind),
            })
            .collect(),
        stars: StarFilter::ALL
            .into_iter()
            .map(|filter| Choice {
                key: filter.key(),
                label: filter.label(),
                selected: filters.starred == Some(filter),
            })
            .collect(),
        search: filters.search.clone(),
        total: roster.total,
        rows,
        previous: (filters.page > 1).then(|| filters.href(&here, filters.page - 1)),
        next: (filters.page < pages).then(|| filters.href(&here, filters.page + 1)),
        page: filters.page,
        pages,
        message_path: message_path(&slug),
        here: here.clone(),
    };

    let layout = StandardLayout::new(here, content)
        .prepare(Some(user))
        .await?;

    ParticipantsPage {
        head: Head::default().prepare(None).await?,
        title: format!("{} Volunteer Manager", project.name),
        layout,
    }
    .into_response(None)
    .await
}

/// The roster's rows, with what the other services know of each person.
async fn rows(
    slug: &str,
    project_id: Ulid,
    participants: Vec<Participant>,
) -> Result<Vec<RosterRow>, Error> {
    let user_ids: Vec<Ulid> = participants.iter().map(|p| p.user_id).collect();
    if user_ids.is_empty() {
        return Ok(Vec::new());
    }

    let (contributions, contacted) = tokio::try_join!(
        request::<Vec<ParticipantContributions>>(
            "log.project_participants",
            ContributionsRequest {
                project_id,
                user_ids: user_ids.clone(),
            },
        ),
        request::<Vec<LastContacted>>(
            "forum.messaging.last_contacted",
            LastContactedRequest {
                from_id: project_id,
                to_ids: user_ids,
            },
        ),
    )?;

    let contributions: HashMap<Ulid, ParticipantContributions> =
        contributions.into_iter().map(|c| (c.user_id, c)).collect();
    let contacted: HashMap<Ulid, String> =
        contacted.into_iter().map(|c| (c.user_id, c.when)).collect();

    Ok(participants
        .into_iter()
        .map(|participant| {
            let contributed = contributions.get(&participant.user_id);
            RosterRow {
                profile: format!("/profile/{}", participant.user_id),
                kind: participant.kind.label(),
                since: date(&participant.since),
                contacted_id: contacted_id(participant.user_id),
                contacted: contacted.get(&participant.user_id).map(|when| date(when)),
                activity: date(contributed.map_or(&participant.since, |c| &c.last)),
                consented: participant.consented.as_deref().map(date),
                contributions: thousands(contributed.map_or(0, |c| c.contributions)),
                star: StarButton::of(slug, &participant),
                user_id: participant.user_id,
                name: participant.name,
            }
        })
        .collect())
}

/// `when` as the page shows a day.
fn date(when: &str) -> String {
    match when.parse::<Timestamp>() {
        Ok(timestamp) => format_date(timestamp.to_zoned(jiff::tz::TimeZone::UTC).date()),
        Err(_) => when.to_string(),
    }
}

/// `POST /projects/<slug>/participants/<kind>/<user>/star`
#[handler]
pub async fn star(
    Path((slug, kind, user_id)): Path<(String, String, Ulid)>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    set_star(slug, kind, user_id, user_info, true).await
}

/// `DELETE /projects/<slug>/participants/<kind>/<user>/star`
#[handler]
pub async fn unstar(
    Path((slug, kind, user_id)): Path<(String, String, Ulid)>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    set_star(slug, kind, user_id, user_info, false).await
}

async fn set_star(
    slug: String,
    kind: String,
    user_id: Ulid,
    user_info: Option<Arc<BasicInfo>>,
    starred: bool,
) -> Result<Response, Error> {
    let project_id = match gate(&slug, &path(&slug), user_info).await? {
        Gate::Open { project_id, .. } => project_id,
        Gate::Closed(response) => return Ok(response),
    };

    let Some(kind) = ParticipantKind::from_key(&kind) else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let on_roster: bool = request(
        "user.roster.star",
        StarRequest {
            project_id,
            user_id,
            kind,
            starred,
        },
    )
    .await?;

    if !on_roster {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }

    let button = StarButton {
        id: star_id(kind, user_id),
        path: star_path(&slug, kind, user_id),
        starred,
    };

    Ok(Html(button.render_once()?).into_response())
}

/// What the page's message form sends.
#[derive(Debug, Deserialize)]
pub struct MessageForm {
    #[serde(default, rename = "to", deserialize_with = "deserialize_string_or_vec")]
    to: Vec<String>,
    subject: String,
    body: String,
}

/// `POST /projects/<slug>/participants/message`: write to the people picked
/// from the roster, as the project.
#[handler]
pub async fn message(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
    QsForm(form): QsForm<MessageForm>,
) -> Result<Response, Error> {
    let project_id = match gate(&slug, &path(&slug), user_info).await? {
        Gate::Open { project_id, .. } => project_id,
        Gate::Closed(response) => return Ok(response),
    };

    if form.subject.trim().is_empty() || form.body.trim().is_empty() {
        return result("A message needs a subject and a body.");
    }

    let picked = form
        .to
        .iter()
        .map(|id| id.parse::<Ulid>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow!("Invalid participant: {e}"))?;

    if picked.is_empty() {
        return result("Pick the people to write to from the list first.");
    }

    // Only people on the roster, whatever the form says.
    let recipients: Vec<Ulid> = request(
        "user.roster.members",
        RosterMembersRequest {
            project_id,
            user_ids: picked,
        },
    )
    .await?;

    if recipients.is_empty() {
        return result("None of the people picked are on the list any longer.");
    }

    let sent: SendBulkResponse = request(
        "forum.messaging.send_bulk",
        SendBulkMessage {
            from_id: project_id,
            to_ids: recipients.clone(),
            subject: form.subject,
            body: form.body,
            pre_approved: true,
        },
    )
    .await?;

    let today = format_date(Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date());
    let mut html = result_html(&format!(
        "Sent to {} {}.",
        thousands(sent.sent),
        if sent.sent == 1 { "person" } else { "people" },
    ))?;
    for user_id in recipients {
        html.push_str(
            &ContactedCell {
                id: contacted_id(user_id),
                contacted: Some(today.clone()),
                oob: true,
            }
            .render_once()?,
        );
    }

    Ok(Html(html).into_response())
}

fn result(note: &str) -> Result<Response, Error> {
    Ok(Html(result_html(note)?).into_response())
}

fn result_html(note: &str) -> Result<String, Error> {
    Ok(ResultNote {
        message: note.to_string(),
    }
    .render_once()?)
}

/// What became of a message sent from the page.
#[derive(TemplateOnce)]
#[template(path = "pages/project_participants_result.stpl.html")]
struct ResultNote {
    message: String,
}

#[derive(TemplateOnce)]
#[template(path = "pages/project_participants.stpl.html")]
struct ParticipantsPage {
    head: <Head as Component>::Prepared,
    title: String,
    layout: <StandardLayout<ParticipantsContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "pages/project_participants_content.stpl.html")]
pub struct ParticipantsContent {
    nav: EditorNav,
    project_path: String,
    name: String,
    is_affiliate: bool,
    asks_consent: bool,
    kinds: Vec<Choice>,
    stars: Vec<Choice>,
    search: String,
    total: u32,
    rows: Vec<RosterRow>,
    previous: Option<String>,
    next: Option<String>,
    page: u32,
    pages: u32,
    message_path: String,
    here: String,
}

/// One option of a filter's menu.
struct Choice {
    key: &'static str,
    label: &'static str,
    selected: bool,
}

struct RosterRow {
    user_id: Ulid,
    name: String,
    profile: String,
    kind: &'static str,
    since: String,
    contacted_id: String,
    contacted: Option<String>,
    activity: String,
    consented: Option<String>,
    contributions: String,
    star: StarButton,
}

fn star_id(kind: ParticipantKind, user_id: Ulid) -> String {
    format!("pm-star-{}-{user_id}", kind.key())
}

/// The button that stars or unstars someone on the roster.
#[derive(TemplateOnce)]
#[template(path = "pages/project_participants_star.stpl.html")]
struct StarButton {
    id: String,
    path: String,
    starred: bool,
}

impl StarButton {
    fn of(slug: &str, participant: &Participant) -> StarButton {
        StarButton {
            id: star_id(participant.kind, participant.user_id),
            path: star_path(slug, participant.kind, participant.user_id),
            starred: participant.starred,
        }
    }
}

/// When the project last wrote to someone; with `oob`, sent out of band to
/// bring the roster's cell up to date.
#[derive(TemplateOnce)]
#[template(path = "pages/project_participants_contacted.stpl.html")]
struct ContactedCell {
    id: String,
    contacted: Option<String>,
    oob: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    #[test]
    fn filters_survive_paging_and_drop_what_is_not_a_filter() {
        let filters = Filters::of(RosterQuery {
            kind: "saved".into(),
            starred: "nonsense".into(),
            q: " Ada & co ".into(),
            page: Some(2),
        });

        check!(
            filters.href("/projects/x/participants", 3)
                == "/projects/x/participants?kind=saved&q=Ada%20%26%20co&page=3"
        );
        check!(
            filters.href("/projects/x/participants", 1)
                == "/projects/x/participants?kind=saved&q=Ada%20%26%20co"
        );
    }

    #[test]
    fn an_unfiltered_first_page_is_the_bare_path() {
        let filters = Filters::of(RosterQuery::default());

        check!(filters.page == 1);
        check!(filters.href("/projects/x/participants", 1) == "/projects/x/participants");
    }

    #[test]
    fn days_are_shown_in_words() {
        check!(date("2026-11-16T15:15:03.171419Z") == "16 November 2026");
        check!(date("not a time") == "not a time");
    }
}
