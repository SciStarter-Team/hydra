//! GET `/staff/curated-report/:slug.csv` — how the people following one
//! curated page have participated in the projects on it. Ported from orrery's
//! `library_staff_report` (`/l/<slug>.csv`).
//!
//! orrery served this from a blob that a nightly Celery task precomputed into
//! `Misc`, which meant the report was as stale as the last run and returned a
//! 404 for any page the task had not reached yet. hydra computes it on request
//! instead: three service calls, none of which depend on a scheduled job
//! having run.
//!
//! A "curated page" is an entity list (see [`crate::staff::project_locations`]
//! for why), and its followers are the people tagged `library:<slug>` — the
//! tag orrery applies when someone takes part through that page.

use std::collections::HashMap;

use common::{
    message::{
        log::{ParticipationByUsersRequest, ParticipationByUsersResponse},
        user::{
            JoinCountsByUsersRequest, JoinCountsByUsersResponse, TagCohortMember, TagCohortRequest,
        },
    },
    service::request,
    shared::db,
};
use poem::{Response, handler, http::StatusCode, web::Path};

use super::csv_field;

/// The tag that marks someone as following a curated page.
///
/// Kept as orrery spells it, because the tags themselves are synced across and
/// a different spelling here would silently report nobody.
fn follower_tag(slug: &str) -> String {
    format!("library:{slug}")
}

/// GET `/staff/curated-report/:slug.csv`.
#[handler]
#[tracing::instrument]
pub async fn report(Path(slug): Path<String>) -> Result<Response, anyhow::Error> {
    // The route captures the whole final segment, `.csv` included.
    let slug = slug.strip_suffix(".csv").unwrap_or(&slug).to_string();

    struct ListRow {
        list_id: String,
        name: String,
    }

    let list = sqlx::query_as!(
        ListRow,
        r#"SELECT list_id AS "list_id!", name AS "name!"
           FROM entity_lists WHERE slug = ?"#,
        slug
    )
    .fetch_optional(db())
    .await?;

    let Some(list) = list else {
        return Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .content_type("text/plain; charset=utf-8")
            .body(format!("No curated page has the slug {slug}.")));
    };

    // The projects on the page, published only: an unpublished entry is a
    // draft placement, and nobody has been able to reach it.
    struct ProjectRow {
        project_id: String,
        name: String,
    }

    let projects = sqlx::query_as!(
        ProjectRow,
        r#"SELECT p.project_id AS "project_id!", p.name AS "name!"
           FROM entity_list_entries e
           JOIN entities ent ON ent.slug = e.entity_slug AND ent.kind = 'Project'
           JOIN projects p ON p.project_id = ent.target
           WHERE e.list_id = ? AND e.published = 1
           ORDER BY e.priority, p.name"#,
        list.list_id
    )
    .fetch_all(db())
    .await?;

    let followers: Vec<TagCohortMember> = request(
        "user.tags.cohort",
        TagCohortRequest {
            tag: follower_tag(&slug),
        },
    )
    .await?;

    let user_ids: Vec<String> = followers.iter().map(|f| f.user_id.to_string()).collect();
    let project_ids: Vec<String> = projects.iter().map(|p| p.project_id.clone()).collect();

    let joins: JoinCountsByUsersResponse = request(
        "user.joins.count_by_users",
        JoinCountsByUsersRequest {
            user_ids: user_ids.clone(),
        },
    )
    .await?;
    let joins: HashMap<String, i64> = joins.counts.into_iter().collect();

    let participation: ParticipationByUsersResponse = request(
        "log.participation_by_users",
        ParticipationByUsersRequest {
            user_ids,
            project_ids,
        },
    )
    .await?;

    let mut by_person: HashMap<(String, String), i64> = HashMap::new();
    for entry in participation.entries {
        by_person.insert((entry.user_id, entry.project_id), entry.count);
    }

    let mut csv = String::from("email,first_name,last_name,total_joins");
    for project in &projects {
        csv.push(',');
        csv.push_str(&csv_field(&project.name));
    }
    csv.push('\n');

    for follower in &followers {
        let user_id = follower.user_id.to_string();

        csv.push_str(&format!(
            "{},{},{},{}",
            csv_field(&follower.email),
            csv_field(&follower.first_name),
            csv_field(&follower.last_name),
            joins.get(&user_id).copied().unwrap_or(0),
        ));

        for project in &projects {
            let count = by_person
                .get(&(user_id.clone(), project.project_id.clone()))
                .copied()
                .unwrap_or(0);
            csv.push(',');
            csv.push_str(&count.to_string());
        }

        csv.push('\n');
    }

    // The list's name is for staff reading the file; the slug is what names it.
    tracing::debug!(list = %list.name, followers = followers.len(), "curated page report");

    Ok(Response::builder()
        .content_type("text/csv")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{slug}.csv\""),
        )
        .body(csv))
}
