//! GET `/staff/import-review` and GET/POST `/staff/import-review/origin` —
//! staff review of newly imported projects, one origin at a time. Ported
//! from orrery's `orrery/staff/import_review.py`; the rules here (what counts
//! as an import, what is held back) must stay identical to it.
//!
//! Partner catalogs (CitSci.org, Anecdata, the Atlas of Living Australia and
//! the like) send projects in batches, and none of them is ever published
//! automatically. Rather than publishing each one from its admin page, a
//! reviewer looks at a random sample of what an origin has sent since a given
//! date and, if the sample is sound, publishes the rest of the batch at once.
//!
//! Projects created by hand on SciStarter carry the origin "SciStarter", and
//! are never approved this way.

use std::collections::{BTreeMap, HashSet};
use std::str::FromStr;

use anyhow::Error;
use common::{shared::db, timestring::TimeString};
use jiff::{Timestamp, civil::Date, tz::TimeZone};
use poem::{
    Response, handler,
    http::StatusCode,
    web::{Form, Query},
};
use rand::seq::SliceRandom;
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::{
    contact::contact_person_email,
    middleware::authorization::Authenticated,
    peering::LoadStorePeered,
    project::{ProjectRejectionReason, publish::publish},
    search::description_excerpt,
    thumbnail::thumbnail_url,
};

/// How many projects the origin page shows in full for review.
const SAMPLE_SIZE: usize = 8;

/// How far back the page looks for a reviewer who has never visited it.
const FIRST_VISIT_LOOKBACK_DAYS: i64 = 30;

/// How much of a sampled project's description the page shows.
const DESCRIPTION_EXCERPT: usize = 600;

/// Whether a project arrived from a partner catalog. An origin of
/// "SciStarter", in any capitalization, marks a project made by hand. An
/// empty one is unknown, and only a project known to be an import may be
/// approved in bulk.
pub fn is_import(origin: &str) -> bool {
    !origin.is_empty() && !origin.eq_ignore_ascii_case("scistarter")
}

/// A reason an unpublished import is left out of a batch approval. Each is a
/// decision somebody already made about that one project, and publishing the
/// batch must not overrule it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    Rejected,
    Duplicate,
    Flagged,
    Test,
    Draft,
}

impl Hold {
    pub fn label(self) -> &'static str {
        match self {
            Hold::Rejected => "Rejected",
            Hold::Duplicate => "Duplicate",
            Hold::Flagged => "Flagged",
            Hold::Test => "Test",
            Hold::Draft => "Draft",
        }
    }
}

/// An imported project created within the window under review.
#[derive(Debug, Clone)]
pub struct Arrival {
    pub project_id: Ulid,
    pub origin: String,
    pub name: String,
    pub slug: Option<String>,
    pub created: Timestamp,
    pub published: bool,
    pub rejection_reason: ProjectRejectionReason,
    pub duplicate: bool,
    pub flagged: bool,
    pub is_test: bool,
    pub draft: bool,
}

impl Arrival {
    pub fn holds(&self) -> Vec<Hold> {
        [
            (
                self.rejection_reason != ProjectRejectionReason::None,
                Hold::Rejected,
            ),
            (self.duplicate, Hold::Duplicate),
            (self.flagged, Hold::Flagged),
            (self.is_test, Hold::Test),
            (self.draft, Hold::Draft),
        ]
        .into_iter()
        .filter_map(|(holds, hold)| holds.then_some(hold))
        .collect()
    }

    /// Whether a batch approval would publish this project.
    pub fn awaiting(&self) -> bool {
        !self.published && self.holds().is_empty()
    }

    /// Whether this project is unpublished and kept out of a batch approval.
    pub fn held(&self) -> bool {
        !self.published && !self.holds().is_empty()
    }
}

/// Imported projects created in [since, until), oldest first.
///
/// `created` is compared as a time rather than as text, because the stored
/// strings carry differing numbers of fractional digits.
async fn arrivals(since: Timestamp, until: Timestamp) -> Result<Vec<Arrival>, Error> {
    let since = TimeString::from(since);
    let until = TimeString::from(until);

    let rows = sqlx::query!(
        r#"SELECT p.project_id, p.origin, p.name, ent.slug AS "slug?",
                  p.created AS "created: TimeString",
                  COALESCE(ent.publish, 0) AS "published!: bool",
                  p.rejection_reason AS "rejection_reason: ProjectRejectionReason",
                  p.duplicate, p.flagged, p.is_test, p.draft
           FROM projects p
           LEFT JOIN entities ent ON ent.target = p.project_id AND ent.kind = 'Project'
           WHERE unixepoch(p.created, 'subsec') >= unixepoch(?, 'subsec')
             AND unixepoch(p.created, 'subsec') < unixepoch(?, 'subsec')
           ORDER BY unixepoch(p.created, 'subsec')"#,
        since,
        until,
    )
    .fetch_all(db())
    .await?;

    Ok(rows
        .into_iter()
        .filter(|row| is_import(&row.origin))
        .filter_map(|row| {
            let project_id = row
                .project_id
                .parse::<Ulid>()
                .inspect_err(|err| tracing::warn!(row.project_id, ?err, "Project id is not a ULID"))
                .ok()?;
            let created = Timestamp::try_from(row.created)
                .inspect_err(
                    |err| tracing::warn!(%project_id, ?err, "Project created time is unreadable"),
                )
                .ok()?;

            Some(Arrival {
                project_id,
                origin: row.origin,
                name: row.name,
                slug: row.slug,
                created,
                published: row.published,
                rejection_reason: row.rejection_reason,
                duplicate: row.duplicate,
                flagged: row.flagged,
                is_test: row.is_test,
                draft: row.draft,
            })
        })
        .collect())
}

/// One origin's counts among a window's arrivals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginSummary {
    pub origin: String,
    pub new: usize,
    pub awaiting: usize,
    pub held: usize,
    pub published: usize,
    pub latest: Timestamp,
}

/// Per-origin counts of `arrivals`, the origins with the most awaiting
/// approval first.
pub fn summarize(arrivals: &[Arrival]) -> Vec<OriginSummary> {
    let mut by_origin: BTreeMap<&str, OriginSummary> = BTreeMap::new();

    for arrival in arrivals {
        let summary = by_origin
            .entry(&arrival.origin)
            .or_insert_with(|| OriginSummary {
                origin: arrival.origin.clone(),
                new: 0,
                awaiting: 0,
                held: 0,
                published: 0,
                latest: arrival.created,
            });

        summary.new += 1;
        summary.awaiting += usize::from(arrival.awaiting());
        summary.held += usize::from(arrival.held());
        summary.published += usize::from(arrival.published);
        summary.latest = summary.latest.max(arrival.created);
    }

    let mut summaries: Vec<OriginSummary> = by_origin.into_values().collect();
    summaries.sort_by(|a, b| {
        b.awaiting
            .cmp(&a.awaiting)
            .then(b.new.cmp(&a.new))
            .then(a.origin.cmp(&b.origin))
    });
    summaries
}

/// A `since` parameter: a date from the picker (midnight UTC), or the exact
/// moment a link carries forward. None when absent or unreadable.
pub fn parse_since(raw: &str) -> Option<Timestamp> {
    let raw = raw.trim();

    if raw.is_empty() {
        return None;
    }

    Timestamp::from_str(raw).ok().or_else(|| {
        Date::from_str(raw)
            .ok()?
            .to_zoned(TimeZone::UTC)
            .ok()
            .map(|zoned| zoned.timestamp())
    })
}

fn utc(time: Timestamp) -> String {
    time.to_zoned(TimeZone::UTC)
        .strftime("%b %-d, %Y, %H:%M")
        .to_string()
}

fn utc_date(time: Timestamp) -> String {
    time.to_zoned(TimeZone::UTC)
        .strftime("%Y-%m-%d")
        .to_string()
}

fn index_url(since: Timestamp) -> String {
    format!(
        "/staff/import-review?since={}",
        urlencoding::encode(&since.to_string())
    )
}

fn origin_url(origin: &str, since: Timestamp) -> String {
    format!(
        "/staff/import-review/origin?origin={}&since={}",
        urlencoding::encode(origin),
        urlencoding::encode(&since.to_string())
    )
}

fn project_url(slug: &Option<String>) -> String {
    slug.as_ref()
        .map(|slug| format!("/projects/{slug}"))
        .unwrap_or_default()
}

fn admin_url(project_id: Ulid) -> String {
    format!("/-admin/projects/{project_id}")
}

fn bad_request(message: &str) -> Response {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .content_type("text/plain; charset=utf-8")
        .body(message.to_string())
}

fn html(body: String) -> Response {
    Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(body)
}

/// When a staff member last looked at the import review page.
///
/// The page shows what arrived since the viewer's previous visit unless they
/// ask for a date, so each staff member has their own bookmark. Peered, so
/// the next visit finds it whichever instance serves it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportReviewVisit {
    pub user_id: Ulid,
    pub viewed_at: TimeString,
}

impl ImportReviewVisit {
    async fn previous(user_id: Ulid) -> Result<Option<Timestamp>, Error> {
        let key = user_id.to_string();

        let viewed_at = sqlx::query_scalar!(
            r#"SELECT viewed_at AS "viewed_at: TimeString" FROM import_review_visits WHERE user_id = ?"#,
            key
        )
        .fetch_optional(db())
        .await?;

        Ok(viewed_at.and_then(|time| Timestamp::try_from(time).ok()))
    }
}

impl LoadStorePeered for ImportReviewVisit {
    type DatabaseKey = Ulid;

    const GROUP: &str = "import-review-visit";

    async fn save_to_database(&self) -> Result<(), Error> {
        let user_id = self.user_id.to_string();

        sqlx::query!(
            r#"INSERT INTO import_review_visits (user_id, viewed_at) VALUES (?, ?)
               ON CONFLICT (user_id) DO UPDATE SET viewed_at = excluded.viewed_at"#,
            user_id,
            self.viewed_at,
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn read_from_database(user_id: Ulid) -> Result<Self, Error> {
        let key = user_id.to_string();

        let viewed_at = sqlx::query_scalar!(
            r#"SELECT viewed_at AS "viewed_at: TimeString" FROM import_review_visits WHERE user_id = ?"#,
            key
        )
        .fetch_one(db())
        .await?;

        Ok(ImportReviewVisit { user_id, viewed_at })
    }
}

/// Where the window under review starts, and why.
pub enum SinceSource {
    Chosen,
    PreviousVisit,
    FirstVisit,
}

struct OriginRow {
    origin: String,
    url: String,
    new: usize,
    awaiting: usize,
    held: usize,
    published: usize,
    latest: String,
}

#[derive(TemplateOnce)]
#[template(path = "staff/import_review.stpl.html")]
struct IndexPage {
    since: String,
    since_date: String,
    source: SinceSource,
    lookback_days: i64,
    origins: Vec<OriginRow>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct IndexQuery {
    since: String,
}

/// GET `/staff/import-review` — every origin that has sent projects since the
/// chosen date, which is the reviewer's previous visit unless they pick one.
#[handler]
pub async fn index(
    Query(query): Query<IndexQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    let now = Timestamp::now();

    let previous_visit = ImportReviewVisit::previous(user_info.user_id).await?;

    let (since, source) = match (parse_since(&query.since), previous_visit) {
        (Some(chosen), _) => (chosen, SinceSource::Chosen),
        (None, Some(previous)) => (previous, SinceSource::PreviousVisit),
        (None, None) => (
            now - jiff::SignedDuration::from_hours(24 * FIRST_VISIT_LOOKBACK_DAYS),
            SinceSource::FirstVisit,
        ),
    };

    // Looking at the page is what moves the bookmark, so the next visit
    // starts from here whichever window this one showed.
    ImportReviewVisit {
        user_id: user_info.user_id,
        viewed_at: now.into(),
    }
    .store()
    .await?;

    let origins = summarize(&arrivals(since, now).await?)
        .into_iter()
        .map(|summary| OriginRow {
            url: origin_url(&summary.origin, since),
            origin: summary.origin,
            new: summary.new,
            awaiting: summary.awaiting,
            held: summary.held,
            published: summary.published,
            latest: utc(summary.latest),
        })
        .collect();

    Ok(html(
        IndexPage {
            since: utc(since),
            since_date: utc_date(since),
            source,
            lookback_days: FIRST_VISIT_LOOKBACK_DAYS,
            origins,
        }
        .render_once()?,
    ))
}

struct Sampled {
    name: String,
    project_url: String,
    admin_url: String,
    url: String,
    image: String,
    added: String,
    status: String,
    location: String,
    contact: String,
    goal: String,
    task: String,
    description: String,
}

impl Sampled {
    async fn load(arrival: &Arrival) -> Result<Sampled, Error> {
        let project_id = arrival.project_id.to_string();

        let row = sqlx::query!(
            r#"SELECT status, location_type, city, state, country, goal, task,
                      description, image, url
               FROM projects WHERE project_id = ?"#,
            project_id
        )
        .fetch_one(db())
        .await?;

        let place = [row.city, row.state, row.country]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(", ");

        let location = match (row.location_type.is_empty(), place.is_empty()) {
            (_, true) => row.location_type,
            (true, false) => place,
            (false, false) => format!("{}: {place}", row.location_type),
        };

        let image = if row.image.is_empty() {
            String::new()
        } else {
            thumbnail_url(&row.image, 540, 320).await
        };

        Ok(Sampled {
            name: arrival.name.clone(),
            project_url: project_url(&arrival.slug),
            admin_url: admin_url(arrival.project_id),
            url: row.url,
            image,
            added: utc(arrival.created),
            status: row.status,
            location,
            contact: contact_person_email(arrival.project_id).await?,
            goal: row.goal,
            task: row.task,
            description: description_excerpt(&row.description, DESCRIPTION_EXCERPT),
        })
    }
}

struct Listed {
    name: String,
    project_url: String,
    admin_url: String,
}

struct HeldRow {
    name: String,
    admin_url: String,
    reasons: String,
    added: String,
}

#[derive(TemplateOnce)]
#[template(path = "staff/import_review_origin.stpl.html")]
struct OriginPage {
    origin: String,
    since: String,
    since_param: String,
    back_url: String,
    summary: Option<OriginSummary>,
    sample: Vec<Sampled>,
    everything: Vec<Listed>,
    ids: String,
    held: Vec<HeldRow>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct OriginQuery {
    origin: String,
    since: String,
}

/// GET `/staff/import-review/origin` — one origin's arrivals since a date: a
/// random sample of the projects awaiting approval, and the form that
/// publishes all of them.
#[handler]
pub async fn review_origin(Query(query): Query<OriginQuery>) -> Result<Response, Error> {
    let Some(since) = parse_since(&query.since) else {
        return Ok(bad_request("origin and since are required"));
    };
    if query.origin.is_empty() {
        return Ok(bad_request("origin and since are required"));
    }

    let projects: Vec<Arrival> = arrivals(since, Timestamp::now())
        .await?
        .into_iter()
        .filter(|arrival| arrival.origin == query.origin)
        .collect();

    let summary = summarize(&projects).into_iter().next();

    let pending: Vec<&Arrival> = projects
        .iter()
        .filter(|arrival| arrival.awaiting())
        .collect();

    let chosen: Vec<&Arrival> = pending
        .choose_multiple(&mut rand::thread_rng(), SAMPLE_SIZE)
        .copied()
        .collect();

    let mut sample = Vec::with_capacity(chosen.len());
    for arrival in chosen {
        sample.push(Sampled::load(arrival).await?);
    }

    let everything = pending
        .iter()
        .map(|arrival| Listed {
            name: arrival.name.clone(),
            project_url: project_url(&arrival.slug),
            admin_url: admin_url(arrival.project_id),
        })
        .collect();

    let ids = pending
        .iter()
        .map(|arrival| arrival.project_id.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let held = projects
        .iter()
        .filter(|arrival| arrival.held())
        .map(|arrival| HeldRow {
            name: arrival.name.clone(),
            admin_url: admin_url(arrival.project_id),
            reasons: arrival
                .holds()
                .into_iter()
                .map(Hold::label)
                .collect::<Vec<_>>()
                .join(", "),
            added: utc(arrival.created),
        })
        .collect();

    Ok(html(
        OriginPage {
            since: utc(since),
            since_param: since.to_string(),
            back_url: index_url(since),
            origin: query.origin,
            summary,
            sample,
            everything,
            ids,
            held,
        }
        .render_once()?,
    ))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ApproveForm {
    origin: String,
    since: String,
    ids: String,
}

#[derive(TemplateOnce)]
#[template(path = "staff/import_review_published.stpl.html")]
struct PublishedPage {
    origin: String,
    count: usize,
    back_url: String,
}

/// POST `/staff/import-review/origin` — publish the projects the reviewer was
/// shown, as far as they are still awaiting approval.
///
/// The form names the projects rather than the window, so nothing that
/// arrived after the page was drawn, or was held back since, is published
/// unseen.
#[handler]
pub async fn approve(
    Form(form): Form<ApproveForm>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    let Some(since) = parse_since(&form.since) else {
        return Ok(bad_request("origin and since are required"));
    };
    if form.origin.is_empty() {
        return Ok(bad_request("origin and since are required"));
    }

    let Ok(ids) = form
        .ids
        .split(',')
        .filter(|part| !part.is_empty())
        .map(Ulid::from_string)
        .collect::<Result<HashSet<Ulid>, _>>()
    else {
        return Ok(bad_request(
            "ids must be a comma-separated list of project ids",
        ));
    };

    let targets: Vec<Arrival> = arrivals(since, Timestamp::now())
        .await?
        .into_iter()
        .filter(|arrival| {
            arrival.origin == form.origin && ids.contains(&arrival.project_id) && arrival.awaiting()
        })
        .collect();

    for target in &targets {
        publish(target.project_id, user_info.user_id).await?;
    }

    tracing::info!(
        origin = form.origin,
        count = targets.len(),
        reviewer = %user_info.user_id,
        "Published an import batch"
    );

    Ok(html(
        PublishedPage {
            back_url: index_url(since),
            origin: form.origin,
            count: targets.len(),
        }
        .render_once()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    fn arrival(origin: &str, created: &str) -> Arrival {
        Arrival {
            project_id: Ulid::new(),
            origin: origin.to_string(),
            name: "A project".to_string(),
            slug: None,
            created: created.parse().unwrap(),
            published: false,
            rejection_reason: ProjectRejectionReason::None,
            duplicate: false,
            flagged: false,
            is_test: false,
            draft: false,
        }
    }

    #[test]
    fn projects_made_by_hand_are_not_imports() {
        check!(!is_import("SciStarter"));
        check!(!is_import("scistarter"));
        check!(!is_import(""));
        check!(is_import("CitSci.org"));
        check!(is_import("API import by dbailey@mdibl.org"));
    }

    #[test]
    fn a_decision_already_made_holds_a_project_back() {
        let open = arrival("CitSci.org", "2026-10-20T12:00:00Z");
        check!(open.awaiting());
        check!(!open.held());

        let rejected = Arrival {
            rejection_reason: ProjectRejectionReason::Science,
            ..open.clone()
        };
        check!(rejected.holds() == vec![Hold::Rejected]);
        check!(!rejected.awaiting());
        check!(rejected.held());

        let twice = Arrival {
            duplicate: true,
            flagged: true,
            ..open.clone()
        };
        check!(twice.holds() == vec![Hold::Duplicate, Hold::Flagged]);

        let published = Arrival {
            published: true,
            draft: true,
            ..open
        };
        check!(!published.awaiting());
        check!(!published.held());
    }

    #[test]
    fn origins_with_the_most_awaiting_approval_come_first() {
        let mut copy = arrival("CitSci.org", "2026-10-20T12:00:00Z");
        copy.duplicate = true;
        let mut live = arrival("CitSci.org", "2026-10-19T12:00:00Z");
        live.published = true;

        let summaries = summarize(&[
            arrival("Anecdata.org", "2026-10-18T12:00:00Z"),
            arrival("CitSci.org", "2026-10-17T12:00:00.5Z"),
            arrival("CitSci.org", "2026-10-21T08:00:00Z"),
            copy,
            live,
        ]);

        check!(
            summaries
                == vec![
                    OriginSummary {
                        origin: "CitSci.org".to_string(),
                        new: 4,
                        awaiting: 2,
                        held: 1,
                        published: 1,
                        latest: "2026-10-21T08:00:00Z".parse().unwrap(),
                    },
                    OriginSummary {
                        origin: "Anecdata.org".to_string(),
                        new: 1,
                        awaiting: 1,
                        held: 0,
                        published: 0,
                        latest: "2026-10-18T12:00:00Z".parse().unwrap(),
                    },
                ]
        );
    }

    #[test]
    fn since_is_a_date_or_an_exact_moment() {
        check!(parse_since("2026-10-01") == Some("2026-10-01T00:00:00Z".parse().unwrap()));
        check!(
            parse_since("2026-10-14T13:18:05.123456Z")
                == Some("2026-10-14T13:18:05.123456Z".parse().unwrap())
        );
        check!(parse_since("") == None);
        check!(parse_since("last week") == None);
    }

    #[test]
    fn a_moment_survives_the_round_trip_through_a_link() {
        let moment: Timestamp = "2026-10-14T13:18:05.123456789Z".parse().unwrap();
        check!(parse_since(&moment.to_string()) == Some(moment));
    }
}
