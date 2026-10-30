//! orrery's JSON search API, `GET /p/finder`, kept for the partners and
//! scripts written against it. The /api page documents it as "Search
//! SciStarter's projects".
//!
//! Its parameters are orrery's: `q` (or `phrase`); `lat` and `lng`; `topic`
//! and `activity` as orrery's choice ids, or exact names; `page`; the
//! `projects` and `events` switches, which search projects alone unless told
//! otherwise; `all-status=true` to take in finished projects; and
//! `format=csv`. The search is the finder's own, ten results to a page as
//! orrery gave them, and each result carries orrery's names and shapes for
//! the fields hydra holds.

use std::collections::{BTreeSet, HashMap};

use anyhow::Error;
use common::shared::db;
use poem::{IntoResponse, Request, Response, handler, http::StatusCode, web::Json};
use serde_json::{Map, Value, json};
use url::form_urlencoded;

use super::{FinderQuery, FinderResult, FtsTermStyle, ResultPage, SearchKinds, run_finder_search};
use crate::project::{ProjectStatus, choice_ids};

/// How many results orrery's API gave to a page.
const PAGE_SIZE: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Json,
    Csv,
}

/// A request to the API, read as orrery read it.
#[derive(Debug)]
struct ApiQuery {
    finder: FinderQuery,
    kinds: SearchKinds,
    page: u32,
    format: Format,
}

impl ApiQuery {
    /// Read the query string `query`. A parameter given more than once takes
    /// its last value, as Django's `QueryDict.get` does.
    fn parse(query: &str) -> Self {
        let pairs: Vec<(String, String)> = form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect();
        let last = |key: &str| {
            pairs
                .iter()
                .rev()
                .find(|(held, _)| held == key)
                .map(|(_, value)| value.trim())
        };
        let switch = |key: &str, default: bool| last(key).map_or(default, |value| value == "true");

        // orrery ignored a coordinate it could not read, and one of zero.
        let coordinate = |key: &str| {
            last(key)
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|value| value.is_finite() && *value != 0.0)
        };

        // A choice orrery could not find matched no project, so one that
        // names nothing here is kept as given, and matches none either.
        let choice = |key: &str, choices: &[(i64, &'static str)]| {
            last(key)
                .filter(|value| !value.is_empty())
                .map(|value| {
                    let named = match value.parse::<i64>() {
                        Ok(id) => Value::from(id),
                        Err(_) => Value::from(value),
                    };

                    choice_ids::named(choices, &named)
                        .unwrap_or(value)
                        .to_string()
                })
                .into_iter()
                .collect::<Vec<String>>()
        };

        let words = last("q")
            .filter(|q| !q.is_empty())
            .or_else(|| last("phrase"))
            .filter(|q| !q.is_empty());

        let finder = FinderQuery {
            q: words.map(str::to_string),
            lat: coordinate("lat"),
            lng: coordinate("lng"),
            topic: choice("topic", choice_ids::TOPICS),
            activity: choice("activity", choice_ids::ACTIVITIES),
            show_completed: switch("all-status", false).then(|| "on".to_string()),
            ..Default::default()
        };

        let kinds = match (switch("projects", true), switch("events", false)) {
            (true, true) => SearchKinds::Both,
            (true, false) => SearchKinds::ProjectsOnly,
            (false, true) => SearchKinds::EventsOnly,
            (false, false) => SearchKinds::Neither,
        };

        let page = last("page")
            .and_then(|page| page.parse::<u32>().ok())
            .unwrap_or(1)
            .max(1);

        let format = match last("format") {
            Some("csv") => Format::Csv,
            _ => Format::Json,
        };

        ApiQuery {
            finder,
            kinds,
            page,
            format,
        }
    }
}

#[handler]
pub async fn handle(req: &Request) -> Result<Response, poem::Error> {
    let query = ApiQuery::parse(req.uri().query().unwrap_or(""));

    let found = run_finder_search(
        &query.finder,
        ResultPage {
            number: query.page,
            size: PAGE_SIZE,
        },
        FtsTermStyle::Phrase,
        query.kinds,
    )
    .await
    .map_err(internal)?;

    let entities = entity_records(&found.results).await.map_err(internal)?;

    match query.format {
        Format::Json => Ok(Json(json!({
            "name": "Matches",
            "total": found.total,
            "page": query.page,
            "entities": entities,
        }))
        .into_response()),

        // orrery answered a page past the end with a 404 rather than an
        // empty file.
        Format::Csv if entities.is_empty() => Ok(StatusCode::NOT_FOUND
            .with_body("No data for page")
            .into_response()),

        Format::Csv => Ok(csv_page(&entities)
            .map_err(internal)?
            .with_content_type("text/csv; charset=utf-8")
            .with_header(
                "Content-Disposition",
                format!("attachment; filename=\"page-{}.csv\"", query.page),
            )
            .into_response()),
    }
}

fn internal(err: impl std::fmt::Display) -> poem::Error {
    tracing::error!("/p/finder search error: {err}");
    poem::Error::from_string(err.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
}

/// The search's results, each as orrery's API described it, in the search's
/// order.
async fn entity_records(results: &[FinderResult]) -> Result<Vec<Map<String, Value>>, Error> {
    let slugs = |kind: &str| -> Result<String, Error> {
        Ok(serde_json::to_string(
            &results
                .iter()
                .filter(|result| result.entity_type == kind)
                .map(|result| result.slug.as_str())
                .collect::<Vec<_>>(),
        )?)
    };

    let mut records = project_records(&slugs("Project")?).await?;
    records.extend(event_records(&slugs("Event")?).await?);

    Ok(results
        .iter()
        .filter_map(|result| {
            let mut record = records.remove(&(result.entity_type.clone(), result.slug.clone()))?;
            record.insert("link".into(), json!(result.link()));

            if result.entity_type == "Event" {
                record.insert("begin".into(), json!(result.event_begin));
            }

            Some(record)
        })
        .collect())
}

type Records = HashMap<(String, String), Map<String, Value>>;

/// The fields every record carries, under orrery's names.
fn common_fields(kind: &str, slug: &str, name: &str) -> Map<String, Value> {
    let mut record = Map::new();
    record.insert("type".into(), json!(kind));
    record.insert("slug".into(), json!(slug));
    record.insert("name".into(), json!(name));
    record.insert("value".into(), json!(name));
    record.insert("publish".into(), json!(true));
    record
}

/// A point as orrery gave it, longitude first.
fn point(lng: Option<f64>, lat: Option<f64>) -> Value {
    match (lng, lat) {
        (Some(lng), Some(lat)) => json!([lng, lat]),
        _ => Value::Null,
    }
}

/// A choice field as orrery gave it: each value's id and name.
fn choices(choices: &[(i64, &'static str)], names: &[String]) -> Value {
    Value::Array(
        names
            .iter()
            .map(|name| json!({"value": choice_ids::id_of(choices, name), "label": name}))
            .collect(),
    )
}

fn status(status: ProjectStatus) -> Value {
    json!({"value": status.orrery_code(), "label": status.label()})
}

/// The projects among `slugs`, a JSON array, by kind and slug.
async fn project_records(slugs: &str) -> Result<Records, Error> {
    let rows = sqlx::query!(
        r#"SELECT
             e.slug AS "slug!",
             p.project_id AS "project_id!",
             p.legacy_v2_project_id,
             p.legacy_v1_project_id,
             p.name, p.url, p.image, p.image_credit, p.presenter, p.goal, p.task,
             p.description, p.origin, p.guid,
             p.status AS "status: ProjectStatus",
             p.how_to_join, p.signup_url, p.special_skills, p.gear,
             p.video_url, p.blog_url, p.twitter_name, p.facebook_page,
             p.address, p.city, p.state, p.zip, p.country,
             p.entirely_online AS "entirely_online: bool",
             p.indoors AS "indoors: bool",
             p.outdoors AS "outdoors: bool",
             p.is_affiliate AS "is_affiliate: bool",
             ST_X(p.point) AS "lng: f64",
             ST_Y(p.point) AS "lat: f64"
           FROM entities e
           JOIN projects p ON p.project_id = e.target
           WHERE e.kind = 'Project'
             AND e.slug IN (SELECT value FROM json_each(?))"#,
        slugs
    )
    .fetch_all(db())
    .await?;

    let ids = serde_json::to_string(&rows.iter().map(|row| &row.project_id).collect::<Vec<_>>())?;
    let topics = project_choices(
        sqlx::query!(
            r#"SELECT project_id AS "project_id!", topic AS "name!" FROM project_topics
               WHERE project_id IN (SELECT value FROM json_each(?)) ORDER BY topic"#,
            ids
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| (row.project_id, row.name)),
    );
    let activities = project_choices(
        sqlx::query!(
            r#"SELECT project_id AS "project_id!", activity AS "name!" FROM project_activities
               WHERE project_id IN (SELECT value FROM json_each(?)) ORDER BY activity"#,
            ids
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| (row.project_id, row.name)),
    );
    let audiences = project_choices(
        sqlx::query!(
            r#"SELECT project_id AS "project_id!", audience AS "name!" FROM project_audiences
               WHERE project_id IN (SELECT value FROM json_each(?)) ORDER BY audience"#,
            ids
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| (row.project_id, row.name)),
    );

    let none = Vec::new();

    Ok(rows
        .into_iter()
        .map(|row| {
            let mut record = common_fields("Project", &row.slug, &row.name);
            record.insert("id".into(), json!(row.legacy_v2_project_id));
            record.insert("legacy_id".into(), json!(row.legacy_v1_project_id));
            record.insert("status".into(), status(row.status));
            record.insert(
                "topics".into(),
                choices(
                    choice_ids::TOPICS,
                    topics.get(&row.project_id).unwrap_or(&none),
                ),
            );
            record.insert(
                "activities".into(),
                choices(
                    choice_ids::ACTIVITIES,
                    activities.get(&row.project_id).unwrap_or(&none),
                ),
            );
            record.insert(
                "audience".into(),
                choices(
                    choice_ids::AUDIENCE,
                    audiences.get(&row.project_id).unwrap_or(&none),
                ),
            );
            record.insert("point".into(), point(row.lng, row.lat));

            for (field, value) in [
                ("url", row.url),
                ("image", row.image),
                ("image_credit", row.image_credit),
                ("presenter", row.presenter),
                ("goal", row.goal),
                ("task", row.task),
                ("description", row.description),
                ("origin", row.origin),
                ("guid", row.guid),
                ("how_to_join", row.how_to_join),
                ("signup_url", row.signup_url),
                ("special_skills", row.special_skills),
                ("gear", row.gear),
                ("video_url", row.video_url),
                ("blog_url", row.blog_url),
                ("twitter_name", row.twitter_name),
                ("facebook_page", row.facebook_page),
                ("address", row.address),
                ("city", row.city),
                ("state", row.state),
                ("zip", row.zip),
                ("country", row.country),
            ] {
                record.insert(field.into(), json!(value));
            }

            for (field, value) in [
                ("entirely_online", row.entirely_online),
                ("indoors", row.indoors),
                ("outdoors", row.outdoors),
                ("is_affiliate", row.is_affiliate),
            ] {
                record.insert(field.into(), json!(value));
            }

            (("Project".to_string(), row.slug), record)
        })
        .collect())
}

/// Each project's names for one choice field.
fn project_choices(rows: impl Iterator<Item = (String, String)>) -> HashMap<String, Vec<String>> {
    let mut by_project: HashMap<String, Vec<String>> = HashMap::new();

    for (project_id, name) in rows {
        by_project.entry(project_id).or_default().push(name);
    }

    by_project
}

/// The events among `slugs`, a JSON array, by kind and slug. An event has no
/// orrery number here, so its `id` is null.
async fn event_records(slugs: &str) -> Result<Records, Error> {
    let rows = sqlx::query!(
        r#"SELECT
             e.slug AS "slug!",
             v.name, v.url, v.image, v.image_credit, v.presenter, v.description,
             v.origin, v.how_to_join,
             v.address, v.city, v.state, v.zip, v.country,
             v.entirely_online AS "entirely_online: bool",
             ST_X(v.point) AS "lng: f64",
             ST_Y(v.point) AS "lat: f64"
           FROM entities e
           JOIN events v ON v.event_id = e.target
           WHERE e.kind = 'Event'
             AND e.slug IN (SELECT value FROM json_each(?))"#,
        slugs
    )
    .fetch_all(db())
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let mut record = common_fields("Event", &row.slug, &row.name);
            record.insert("id".into(), Value::Null);
            record.insert("point".into(), point(row.lng, row.lat));
            record.insert("entirely_online".into(), json!(row.entirely_online));

            for (field, value) in [
                ("url", row.url),
                ("image", row.image),
                ("image_credit", row.image_credit),
                ("presenter", row.presenter),
                ("description", row.description),
                ("origin", row.origin),
                ("how_to_join", row.how_to_join),
                ("address", row.address),
                ("city", row.city),
                ("state", row.state),
                ("zip", row.zip),
                ("country", row.country),
            ] {
                record.insert(field.into(), json!(value));
            }

            (("Event".to_string(), row.slug), record)
        })
        .collect())
}

/// A page of records as CSV: a column for every field any of them has, in
/// alphabetical order as orrery's were. A list or object is written as JSON.
fn csv_page(records: &[Map<String, Value>]) -> Result<String, Error> {
    let columns: BTreeSet<&str> = records
        .iter()
        .flat_map(|record| record.keys().map(String::as_str))
        .collect();

    let mut out = csv::Writer::from_writer(Vec::new());
    out.write_record(&columns)?;

    for record in records {
        out.write_record(columns.iter().map(|column| match record.get(*column) {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(text)) => text.clone(),
            Some(other) => other.to_string(),
        }))?;
    }

    Ok(String::from_utf8(out.into_inner()?)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    #[test]
    fn a_search_is_read_with_orrerys_names_and_defaults() {
        let query = ApiQuery::parse("q=butterfly&lat=35.78&lng=-78.64&topic=13&activity=40&page=2");

        check!(query.finder.q.as_deref() == Some("butterfly"));
        check!(query.finder.lat == Some(35.78));
        check!(query.finder.lng == Some(-78.64));
        check!(query.finder.topic == ["Astronomy & Space"]);
        check!(query.finder.activity == ["On a hike"]);
        check!(query.finder.show_completed.is_none());
        check!(query.kinds == SearchKinds::ProjectsOnly);
        check!(query.page == 2);
        check!(query.format == Format::Json);
    }

    #[test]
    fn phrase_stands_in_for_an_empty_q() {
        check!(ApiQuery::parse("q=&phrase=amateur").finder.q.as_deref() == Some("amateur"));
        check!(
            ApiQuery::parse("q=moths&phrase=amateur")
                .finder
                .q
                .as_deref()
                == Some("moths")
        );
        check!(ApiQuery::parse("").finder.q.is_none());
    }

    #[test]
    fn a_choice_may_be_named_and_one_naming_nothing_matches_nothing() {
        check!(ApiQuery::parse("topic=Birds").finder.topic == ["Birds"]);
        check!(ApiQuery::parse("topic=99999").finder.topic == ["99999"]);
        check!(ApiQuery::parse("topic=").finder.topic.is_empty());
    }

    #[test]
    fn the_last_of_a_repeated_parameter_counts() {
        check!(ApiQuery::parse("topic=13&topic=11").finder.topic == ["Birds"]);
    }

    #[test]
    fn events_are_searched_only_when_asked_for() {
        check!(ApiQuery::parse("events=true").kinds == SearchKinds::Both);
        check!(ApiQuery::parse("events=true&projects=false").kinds == SearchKinds::EventsOnly);
        check!(ApiQuery::parse("projects=false").kinds == SearchKinds::Neither);
    }

    #[test]
    fn all_status_takes_in_finished_projects() {
        check!(
            ApiQuery::parse("all-status=true")
                .finder
                .show_completed
                .as_deref()
                == Some("on")
        );
        check!(
            ApiQuery::parse("all-status=false")
                .finder
                .show_completed
                .is_none()
        );
    }

    #[test]
    fn an_unreadable_or_zero_coordinate_or_page_is_ignored() {
        let query = ApiQuery::parse("lat=north&lng=0&page=-3");

        check!(query.finder.lat.is_none());
        check!(query.finder.lng.is_none());
        check!(query.page == 1);
    }

    #[test]
    fn csv_is_asked_for_by_format() {
        check!(ApiQuery::parse("format=csv").format == Format::Csv);
    }

    #[test]
    fn a_choice_field_carries_orrerys_ids() {
        check!(
            choices(choice_ids::TOPICS, &["Birds".to_string()])
                == json!([{"value": 11, "label": "Birds"}])
        );
    }

    #[test]
    fn a_status_carries_orrerys_number_where_orrery_has_one() {
        check!(status(ProjectStatus::Complete) == json!({"value": 4, "label": "Complete"}));
        check!(status(ProjectStatus::Expired) == json!({"value": null, "label": "Expired"}));
    }

    #[test]
    fn a_csv_page_has_a_column_for_every_field_in_order() {
        let mut project = common_fields("Project", "ebutterfly", "eButterfly");
        project.insert("topics".into(), json!([{"value": 11, "label": "Birds"}]));
        let event = common_fields("Event", "bioblitz", "BioBlitz, \"the\" event");

        let csv = csv_page(&[project, event]).unwrap();
        let lines: Vec<&str> = csv.lines().collect();

        check!(lines[0] == "name,publish,slug,topics,type,value");
        check!(
            lines[1]
                == r#"eButterfly,true,ebutterfly,"[{""label"":""Birds"",""value"":11}]",Project,eButterfly"#
        );
        check!(
            lines[2]
                == r#""BioBlitz, ""the"" event",true,bioblitz,,Event,"BioBlitz, ""the"" event""#
        );
    }
}
