//! Importing the Atlas of Living Australia's BioCollect catalog.
//!
//! BioCollect shares its whole catalog with SciStarter, about 2,200 records,
//! but only the projects that originate at ALA are imported: the rest are
//! ALA's copies of projects from other catalogs (SciStarter's own among
//! them), which reach SciStarter by their own routes.
//!
//! A record is kept to the one SciStarter project it describes by its
//! `projectId`, which the project holds as its `guid`. Failing that, an ALA
//! project of the same name whose guid is no projectId in the catalog is this
//! record imported under an earlier id (ids from the SciStarter 2.0 era, or
//! none at all because an earlier importer could stop before setting it), and
//! is adopted. Any other project with the record's name means the record is
//! not imported: it is the same real project reaching SciStarter another way
//! (BioCollect keeps its own records for iNaturalist, eBird, Redmap,
//! SeagrassWatch and others SciStarter already lists), or another BioCollect
//! record of the same name.
//!
//! [`AlaImport`] remembers which version of each record was last imported in
//! full, so a record is imported again only when BioCollect changes it.
//! Imports write with [`FieldSource::Api`], so fields a project's own people
//! or SciStarter's staff have edited are left alone.
//!
//! This is a port of orrery's `load_ALA_from_web`. orrery imports until hydra
//! is the primary, and the project sync carries its import state here, so the
//! task only runs in a primary build. Every frontend instance schedules it,
//! and one of them is elected to run each import.

use std::collections::HashSet;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Error, anyhow};
use common::{
    elect,
    message::{
        log::PageChange,
        user::{UserByEmailRequest, UserByEmailResponse},
    },
    primary::PRIMARY,
    service::request,
    shared::db,
    timestring::TimeString,
};
use jiff::Timestamp;
use serde::Deserialize;
use serde_json::Value;
use tokio::time::sleep;
use ulid::Ulid;

use super::{
    ProjectStatus,
    all::{ProjectAll, Submission, create_page_entity, persist_new_project},
    field::{FIELDS_OF_SCIENCE_VOCABULARY, ProjectFieldType},
    field_source::FieldSource,
    location::{ANYWHERE, IN_A_RANGE},
};
use crate::{
    contact::{edit_manager, remove_manager},
    page_change::Stored,
    peering::LoadStorePeered,
};

/// The `origin` of a project that originates at ALA, in BioCollect's records
/// and in ours.
pub const ORIGIN: &str = "atlasoflivingaustralia";

const FEED: &str = "https://biocollect.ala.org.au/ws/project/search?initiator=scistarter";

/// Records fetched per request.
const PAGE: usize = 100;

/// How often the catalog is imported, as orrery's beat entry does. Imports
/// fall due at whole multiples of this since the epoch, so that every
/// instance agrees on when, however recently it started.
const INTERVAL: Duration = Duration::from_secs(60 * 60 * 24 * 3);

/// The account imported projects belong to until someone claims them.
const OWNER_EMAIL: &str = "info@scistarter.org";

const CHILDREN_AND_ADULTS: &[&str] = &[
    "Elementary school (6 - 10 years)",
    "Middle school (11 - 13 years)",
    "High school (14 - 17 years)",
    "Adults",
    "Families",
];
const ADULTS: &[&str] = &["Adults"];

/// Import the catalog every three days, for as long as this build is the
/// primary.
pub async fn import_periodically() {
    if !PRIMARY {
        tracing::info!(
            "ALA import: orrery is still the primary, so not importing. \
             See crate::project::ala for the cutover."
        );
        return;
    }

    loop {
        let interval = INTERVAL.as_secs() as i64;
        let slot = Timestamp::now().as_second() / interval + 1;
        let due = Timestamp::from_second(slot * interval).unwrap_or_else(|_| Timestamp::now());
        let wait = (due - Timestamp::now())
            .total(jiff::Unit::Second)
            .unwrap_or(0.0)
            .max(0.0);
        sleep(Duration::from_secs_f64(wait)).await;

        let elected = elect::elect_instance(
            "ALA_IMPORT_CLAIMS",
            &slot.to_string(),
            &Ulid::new().to_string(),
            INTERVAL,
        )
        .await;

        match elected {
            Ok(true) => {
                tracing::info!("ALA import: elected to import");
                if let Err(err) = import().await {
                    tracing::error!("ALA import: {err:#}");
                }
            }
            Ok(false) => {}
            Err(err) => tracing::error!("ALA import: the election failed: {err:#}"),
        }
    }
}

/// Import every record of the catalog that is new or has changed.
///
/// Each record is imported on its own: one that fails is reported and the
/// rest are still imported, and the import then fails naming every record
/// that did.
pub async fn import() -> Result<(), Error> {
    // The whole catalog is read before anything is imported, because
    // matching a record by name has to know which projectIds the catalog
    // still holds.
    let entries = fetch_catalog().await?;

    let feed_ids: HashSet<String> = entries
        .iter()
        .filter_map(|entry| entry.get("projectId")?.as_str())
        .filter(|id| !id.is_empty())
        .map(String::from)
        .collect();

    let owner = owner().await?;
    let mut failed = Vec::new();

    for entry in entries {
        let outcome = match serde_json::from_value::<Record>(entry.clone()) {
            Ok(record) => import_record(&record, &feed_ids, owner).await,
            Err(err) => Err(err.into()),
        };

        if let Err(err) = outcome {
            let id = entry.get("projectId").and_then(Value::as_str).unwrap_or("");
            let name = entry.get("name").and_then(Value::as_str).unwrap_or("");
            tracing::error!("ALA import: BioCollect project {id} ({name}) failed: {err:#}");
            failed.push(format!("{id} ({name})"));
        }
    }

    if failed.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "{} BioCollect project(s) failed to import: {}",
            failed.len(),
            failed.join(", ")
        ))
    }
}

async fn fetch_catalog() -> Result<Vec<Value>, Error> {
    #[derive(Deserialize)]
    struct Page {
        projects: Vec<Value>,
    }

    let client = reqwest::Client::new();
    let mut entries = Vec::new();

    for offset in (0..).step_by(PAGE) {
        let response = client
            .get(FEED)
            .query(&[
                ("max", PAGE.to_string()),
                ("offset", offset.to_string()),
                ("sort", "dateCreatedSort".to_string()),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "BioCollect search returned HTTP {} at offset {offset}",
                response.status()
            ));
        }

        let page: Page = response.json().await?;
        if page.projects.is_empty() {
            break;
        }

        entries.extend(page.projects);
    }

    Ok(entries)
}

async fn owner() -> Result<Ulid, Error> {
    let found: UserByEmailResponse = request(
        "user.search.by_email",
        UserByEmailRequest {
            email: OWNER_EMAIL.to_string(),
        },
    )
    .await?;

    let user_id = found
        .user_id
        .filter(|_| found.found)
        .ok_or_else(|| anyhow!("no account has the address {OWNER_EMAIL}"))?;

    Ok(Ulid::from_string(&user_id)?)
}

/// A BioCollect catalog record, as much of it as the import reads.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Record {
    project_id: Option<String>,
    name: Option<String>,
    origin: Option<String>,
    last_updated: Option<String>,
    description: Option<String>,
    aim: Option<String>,
    task: Option<String>,
    url_web: Option<String>,
    url: Option<String>,
    url_image: Option<String>,
    logo_credit: Option<String>,
    organisation_name: Option<String>,
    manager_email: Option<String>,
    difficulty: Option<String>,
    is_suitable_for_children: Option<bool>,
    #[serde(rename = "isDIY")]
    is_diy: Option<bool>,
    start_date: Option<String>,
    planned_start_date: Option<String>,
    end_date: Option<String>,
    planned_end_date: Option<String>,
    equipment: Option<String>,
    get_involved: Option<String>,
    keywords: Option<String>,
    science_type: Option<Value>,
    links: Option<Vec<Link>>,
    coverage: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Link {
    role: String,
    url: String,
}

impl Record {
    fn text(field: &Option<String>) -> String {
        field.clone().unwrap_or_default()
    }

    fn name(&self) -> String {
        self.name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "[name missing!]".to_string())
    }

    /// The science types, which BioCollect sends as a list or a single
    /// string.
    fn science_types(&self) -> Vec<String> {
        match &self.science_type {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect(),
            Some(Value::String(single)) => vec![single.clone()],
            _ => Vec::new(),
        }
    }

    /// The tags the record suggests: its keywords and science types.
    fn tags(&self) -> Vec<String> {
        self.keywords
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(str::to_string)
            .chain(self.science_types())
            .map(|tag| tag.trim().to_string())
            .filter(|tag| !tag.is_empty())
            .collect()
    }

    fn difficulty(&self) -> &'static str {
        match self.difficulty.as_deref() {
            Some("Medium") => "Medium",
            Some("Hard") => "Difficult",
            _ => "Easy",
        }
    }

    fn audience(&self) -> &'static [&'static str] {
        if self.is_suitable_for_children == Some(true) {
            CHILDREN_AND_ADULTS
        } else {
            ADULTS
        }
    }

    fn begin(&self) -> Option<Timestamp> {
        parse_time(self.start_date.as_deref()).or(parse_time(self.planned_start_date.as_deref()))
    }

    fn end(&self) -> Option<Timestamp> {
        parse_time(self.end_date.as_deref()).or(parse_time(self.planned_end_date.as_deref()))
    }

    fn facebook_page(&self) -> Option<String> {
        self.links
            .iter()
            .flatten()
            .filter(|link| link.role == "facebook")
            .map(|link| link.url.clone())
            .last()
    }
}

/// A BioCollect time: ISO 8601 in UTC, written with or without its `Z`.
fn parse_time(text: Option<&str>) -> Option<Timestamp> {
    let text = text.filter(|text| !text.is_empty())?;

    Timestamp::from_str(text).ok().or_else(|| {
        jiff::civil::DateTime::from_str(text)
            .ok()?
            .to_zoned(jiff::tz::TimeZone::UTC)
            .ok()
            .map(|zoned| zoned.timestamp())
    })
}

/// Bring the SciStarter project a record describes up to date, creating it
/// if there is none and the record should be imported.
async fn import_record(
    record: &Record,
    feed_ids: &HashSet<String>,
    owner: Ulid,
) -> Result<(), Error> {
    if record.origin.as_deref() != Some(ORIGIN) {
        return Ok(());
    }

    if record
        .url_web
        .as_deref()
        .unwrap_or("")
        .contains("scistarter")
    {
        return Ok(());
    }

    let Some(project_id) = record.project_id.as_deref().filter(|id| !id.is_empty()) else {
        return Ok(());
    };

    let updated = parse_time(record.last_updated.as_deref());

    let (mut project, adopted) = match find(project_id, record, feed_ids).await? {
        Found::Imported(id) => (ProjectAll::load(id).await?, false),
        Found::Adoptable(id) => (ProjectAll::load(id).await?, true),
        Found::NamedElsewhere => return Ok(()),
        Found::Nothing => {
            let mut project = ProjectAll::default();
            project.project_id = Ulid::new();
            project.created = TimeString::now();
            project.guid = project_id.to_string();
            project.location_type = ANYWHERE.to_string();
            project.page_creator_id = owner;
            return write(project, record, updated, owner, true).await;
        }
    };

    if adopted {
        project.guid = project_id.to_string();
    } else if let (Some(updated), Some(imported)) =
        (updated, AlaImport::last_updated(project.project_id).await?)
    {
        if imported >= updated {
            return Ok(());
        }
    }

    write(project, record, updated, owner, false).await
}

/// What the projects already held make of a record.
enum Found {
    /// The project imported from this record.
    Imported(Ulid),
    /// An ALA project of the record's name that no record in the catalog
    /// claims, taken to be this record imported under an earlier id.
    Adoptable(Ulid),
    /// A project of the record's name that is not this record's.
    NamedElsewhere,
    Nothing,
}

async fn find(
    project_id: &str,
    record: &Record,
    feed_ids: &HashSet<String>,
) -> Result<Found, Error> {
    // Several projects can carry one record's id, copies left when the
    // record was renamed. The published one, then the oldest, is the one
    // kept up to date.
    let imported = sqlx::query_scalar!(
        r#"SELECT p.project_id AS "project_id!"
           FROM projects p LEFT JOIN entities e ON e.target = p.project_id
           WHERE p.guid = ? AND p.origin = ?
           ORDER BY coalesce(e.publish, 0) DESC, p.created, p.project_id
           LIMIT 1"#,
        project_id,
        ORIGIN,
    )
    .fetch_optional(db())
    .await?;

    if let Some(id) = imported {
        return Ok(Found::Imported(id.parse()?));
    }

    let Some(name) = record.name.as_deref().filter(|name| !name.is_empty()) else {
        return Ok(Found::Nothing);
    };

    let namesakes = sqlx::query!(
        r#"SELECT p.project_id AS "project_id!", p.origin AS "origin!", p.guid AS "guid!"
           FROM projects p LEFT JOIN entities e ON e.target = p.project_id
           WHERE p.name = ?
           ORDER BY coalesce(e.publish, 0) DESC, p.created, p.project_id"#,
        name,
    )
    .fetch_all(db())
    .await?;

    let adoptable = namesakes
        .iter()
        .find(|p| p.origin == ORIGIN && !feed_ids.contains(&p.guid));

    Ok(match adoptable {
        Some(p) => Found::Adoptable(p.project_id.parse()?),
        None if namesakes.is_empty() => Found::Nothing,
        None => Found::NamedElsewhere,
    })
}

/// Write the record onto the project, and remember the version imported.
///
/// Everything that can fail on another server (the image, the coverage) is
/// settled before anything is written. What is written goes in three steps
/// (the project, its page, its import state), and the import state goes last,
/// so a record that fails partway is imported again on the next run, which
/// finds the project by its guid and supplies a page if it has none.
async fn write(
    mut project: ProjectAll,
    record: &Record,
    updated: Option<Timestamp>,
    owner: Ulid,
    is_new: bool,
) -> Result<(), Error> {
    project.draft = false;
    project.welcomed = true;
    project.origin = ORIGIN.to_string();
    project.suborigin = Record::text(&record.origin);
    project.audience = record.audience().iter().map(|a| a.to_string()).collect();
    project.description = Record::text(&record.description);
    project.difficulty = record.difficulty().to_string();
    match record
        .manager_email
        .as_deref()
        .filter(|email| !email.is_empty())
    {
        Some(email) => edit_manager(&mut project.contacts, |m| m.email = email.to_string()),
        None => remove_manager(&mut project.contacts),
    }

    // The status follows the dates the project ends up with, which are its
    // own where they are protected from imports.
    let protected = project.get_protected_fields(FieldSource::Api).await?;
    let held =
        |time: &Option<TimeString>| time.clone().and_then(|time| Timestamp::try_from(time).ok());
    let begin = match protected.contains(&ProjectFieldType::Begin) {
        true => held(&project.begin),
        false => record.begin(),
    };
    let end = match protected.contains(&ProjectFieldType::End) {
        true => held(&project.end),
        false => record.end(),
    };

    project.begin = begin.map(TimeString::from);
    project.end = end.map(TimeString::from);

    if project.gear.is_empty() {
        project.gear = Record::text(&record.equipment);
    }

    if project.how_to_join.is_empty() {
        project.how_to_join = Record::text(&record.get_involved);
    }

    project.diy = record.is_diy == Some(true);

    for tag in record.tags() {
        project.tags.insert(tag.into());
    }

    let mut fields: Vec<String> = project.fields_of_science.to_vec();
    for science in record.science_types() {
        if FIELDS_OF_SCIENCE_VOCABULARY.contains(&science.as_str()) && !fields.contains(&science) {
            fields.push(science);
        }
    }
    project.fields_of_science = fields.into_boxed_slice();

    if let Some(page) = record.facebook_page() {
        project.facebook_page = page;
    }

    project.name = record.name();
    project.presenter = Record::text(&record.organisation_name);
    project.url = record
        .url_web
        .clone()
        .filter(|url| !url.is_empty())
        .or_else(|| record.url.clone())
        .unwrap_or_default();
    project.goal = Record::text(&record.aim);
    project.task = Record::text(&record.task);

    let now = Timestamp::now();
    project.status = match (begin, end) {
        (Some(begin), _) if begin > now => ProjectStatus::Pending,
        (_, Some(end)) if end < now => ProjectStatus::Complete,
        _ => ProjectStatus::Active,
    };

    if let Some(image_url) = record.url_image.as_deref().filter(|url| !url.is_empty()) {
        if project.image.is_empty() {
            project.image = crate::upload::rehost::rehost_image_url(image_url, owner)
                .await
                .ok_or_else(|| anyhow!("unable to load the project image {image_url}"))?;
            project.image_credit = Record::text(&record.logo_credit);
        }
    }

    if let Some(coverage) = &record.coverage {
        if !crate::coverage::is_global(coverage) {
            match crate::coverage::multipolygon(db(), &fix_lat_lon(coverage)).await {
                Ok(regions) => {
                    project.regions = Some(regions);
                    project.location_type = IN_A_RANGE.to_string();
                }
                Err(crate::coverage::CoverageError::Database(err)) => {
                    return Err(anyhow!("examining the coverage failed: {err}"));
                }
                Err(err) => {
                    tracing::warn!("ALA import: bad geometry for {}: {err}", project.name);
                }
            }
        }
    }

    project.authoritative = false;
    project.update_source = Some(FieldSource::Api);

    let project_id = project.project_id;

    if is_new {
        persist_new_project(project, false, owner, Submission::Imported).await?;
    } else {
        let name = project.name.clone();
        let before = Stored::of_project(project_id).await;
        project.store().await?;

        let target = project_id.to_string();
        let has_page = sqlx::query_scalar!(
            r#"SELECT 1 AS "found!: i64" FROM entities WHERE target = ?"#,
            target
        )
        .fetch_optional(db())
        .await?
        .is_some();

        if !has_page {
            create_page_entity(project_id, &name, false).await?;
        }

        let after = Stored::of_project(project_id).await;
        before
            .log_update(
                &after,
                PageChange::ProjectEdit,
                None,
                project_id,
                "the Atlas of Living Australia",
            )
            .await?;
    }

    AlaImport {
        project_id,
        last_updated: updated.map(TimeString::from),
    }
    .store()
    .await
}

/// BioCollect coverages sometimes give a point as latitude, longitude. A pair
/// whose first value looks like a latitude and whose second does not is taken
/// to be transposed, particularly since these projects are in and around
/// Australia.
fn fix_lat_lon(node: &Value) -> Value {
    match node {
        Value::Array(items) if items.len() == 2 && items.iter().all(Value::is_number) => {
            let x = items[0].as_f64().unwrap_or_default();
            let y = items[1].as_f64().unwrap_or_default();
            let looks_like_latitude = |v: f64| -90.0 < v && v < 90.0;

            if looks_like_latitude(x) && !looks_like_latitude(y) {
                serde_json::json!([y, x])
            } else {
                serde_json::json!([x, y])
            }
        }
        Value::Array(items) => Value::Array(items.iter().map(fix_lat_lon).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), fix_lat_lon(value)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The BioCollect version a project last took in full.
///
/// Peered, because whichever frontend instance runs the next import has to
/// know what the last one took. Until hydra is the primary, orrery imports
/// and the project sync carries its `bridge.ALAProject` rows here.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlaImport {
    pub project_id: Ulid,
    /// BioCollect's `lastUpdated` for the version taken, None when the
    /// record did not say.
    pub last_updated: Option<TimeString>,
}

impl AlaImport {
    async fn last_updated(project_id: Ulid) -> Result<Option<Timestamp>, Error> {
        let key = project_id.to_string();

        let held = sqlx::query_scalar!(
            r#"SELECT last_updated AS "last_updated: TimeString" FROM ala_projects WHERE project_id = ?"#,
            key
        )
        .fetch_optional(db())
        .await?
        .flatten();

        Ok(held.and_then(|time| Timestamp::try_from(time).ok()))
    }
}

impl LoadStorePeered for AlaImport {
    type DatabaseKey = Ulid;

    const GROUP: &str = "ala-import";

    async fn save_to_database(&self) -> Result<(), Error> {
        let project_id = self.project_id.to_string();

        sqlx::query!(
            r#"INSERT INTO ala_projects (project_id, last_updated) VALUES (?, ?)
               ON CONFLICT (project_id) DO UPDATE SET last_updated = excluded.last_updated"#,
            project_id,
            self.last_updated,
        )
        .execute(db())
        .await?;

        Ok(())
    }

    async fn read_from_database(project_id: Ulid) -> Result<Self, Error> {
        let key = project_id.to_string();

        let last_updated = sqlx::query_scalar!(
            r#"SELECT last_updated AS "last_updated: TimeString" FROM ala_projects WHERE project_id = ?"#,
            key
        )
        .fetch_one(db())
        .await?;

        Ok(AlaImport {
            project_id,
            last_updated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;
    use serde_json::json;

    fn record(fields: Value) -> Record {
        let mut base = json!({
            "projectId": "quoll-count",
            "name": "Quoll Count",
            "origin": ORIGIN,
            "lastUpdated": "2026-10-04T06:44:28Z",
            "organisationName": "Quoll Society",
            "isSuitableForChildren": true,
            "scienceType": ["Biology", "Ecology", "Quolls"],
            "keywords": "quolls, marsupials",
            "links": [{"role": "facebook", "url": "https://facebook.com/quolls"}],
        });
        base.as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        serde_json::from_value(base).unwrap()
    }

    #[test]
    fn a_record_reads_as_orrery_reads_it() {
        let record = record(json!({}));

        check!(record.name() == "Quoll Count");
        check!(record.tags() == ["quolls", "marsupials", "Biology", "Ecology", "Quolls"]);
        check!(record.audience() == CHILDREN_AND_ADULTS);
        check!(record.difficulty() == "Easy");
        check!(record.facebook_page() == Some("https://facebook.com/quolls".to_string()));
        check!(Record::text(&record.organisation_name) == "Quoll Society");
    }

    #[test]
    fn a_record_unsuitable_for_children_is_for_adults() {
        check!(record(json!({"isSuitableForChildren": false})).audience() == ADULTS);
        check!(record(json!({"isSuitableForChildren": null})).audience() == ADULTS);
    }

    #[test]
    fn difficulty_maps_onto_ours() {
        check!(record(json!({"difficulty": "Medium"})).difficulty() == "Medium");
        check!(record(json!({"difficulty": "Hard"})).difficulty() == "Difficult");
        check!(record(json!({"difficulty": null})).difficulty() == "Easy");
    }

    #[test]
    fn a_missing_start_falls_back_to_the_planned_one() {
        let record = record(json!({"startDate": null, "plannedStartDate": "2026-03-29T14:00:00Z"}));

        check!(record.begin() == Some("2026-03-29T14:00:00Z".parse().unwrap()));
    }

    #[test]
    fn times_are_read_with_or_without_their_zone() {
        check!(
            parse_time(Some("2026-10-04T06:44:28Z"))
                == Some("2026-10-04T06:44:28Z".parse().unwrap())
        );
        check!(
            parse_time(Some("2026-10-04T06:44:28"))
                == Some("2026-10-04T06:44:28Z".parse().unwrap())
        );
        check!(parse_time(Some("")) == None);
        check!(parse_time(None) == None);
    }

    #[test]
    fn a_transposed_pair_is_turned_round() {
        let coverage =
            json!({"type": "Polygon", "coordinates": [[[-35.0, 149.0], [149.5, -35.5]]]});

        check!(
            fix_lat_lon(&coverage)
                == json!({"type": "Polygon", "coordinates": [[[149.0, -35.0], [149.5, -35.5]]]})
        );
    }
}
