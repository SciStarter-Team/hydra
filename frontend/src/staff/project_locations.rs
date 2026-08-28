//! GET `/staff/project-locations` — which projects are placed on which curated
//! list, and whether that placement is published. Ported from orrery's
//! `project_locations`.
//!
//! orrery calls the underlying record a `LibraryKit`, whose own verbose name is
//! "Project on a curated page": a project placed on a library's page, in an
//! order, published or not. hydra already has that shape as an entity list —
//! a named, slugged, ordered set of entities with a page of its own at
//! `/list/<slug>` — so the report reads lists rather than a parallel table.
//!
//! Public and private lists are both reported. A private list is one whose
//! page is restricted to its viewers, not one hidden from staff, and staff use
//! this report to find where a project has been placed at all.

use common::shared::db;
use poem::{Response, handler};

use crate::component::html_escape;

/// GET `/staff/project-locations` — the report.
#[handler]
#[tracing::instrument]
pub async fn project_locations() -> Result<Response, anyhow::Error> {
    struct Row {
        list_name: String,
        list_slug: Option<String>,
        private: bool,
        entity_slug: String,
        entity_name: String,
        published: bool,
    }

    // The entity's name comes from the project or event behind it; entities
    // carry only a slug. A placement whose slug names nothing is still
    // reported — with a blank name — because a broken placement is exactly the
    // sort of thing staff run this report to find.
    let rows = sqlx::query_as!(
        Row,
        r#"
        SELECT
            l.name AS "list_name!",
            l.slug AS "list_slug?",
            l.private AS "private!: bool",
            e.entity_slug AS "entity_slug!",
            COALESCE(p.name, ev.name, '') AS "entity_name!",
            e.published AS "published!: bool"
        FROM entity_list_entries e
        JOIN entity_lists l ON l.list_id = e.list_id
        LEFT JOIN entities ent ON ent.slug = e.entity_slug
        LEFT JOIN projects p ON ent.kind = 'Project' AND p.project_id = ent.target
        LEFT JOIN events ev ON ent.kind = 'Event' AND ev.event_id = ent.target
        ORDER BY l.name, e.priority, e.entity_slug
        "#
    )
    .fetch_all(db())
    .await?;

    let placements = rows.len();
    let published = rows.iter().filter(|r| r.published).count();

    let mut html = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <title>Projects on Curated Pages</title>\
         <style>body{font-family:sans-serif;margin:2rem;max-width:960px;}\
         table{border-collapse:collapse;width:100%;margin-top:1rem;}\
         td,th{padding:4px 8px;text-align:left;border-bottom:1px solid #eee;}\
         th{border-bottom:1px solid #ccc;}\
         .draft{color:#888;}.missing{color:#b00;}</style>\
         </head><body><h1>Projects on curated pages</h1>",
    );

    html.push_str(&format!(
        "<p>{placements} placements, {published} of them published.</p>\
         <table><tr><th>List</th><th>Project</th><th>Slug</th><th>Published</th></tr>",
    ));

    for row in &rows {
        // A list with no slug has no page of its own yet; there is nowhere to
        // link, so the name stands alone.
        let list = match &row.list_slug {
            Some(slug) if !slug.is_empty() => format!(
                "<a href=\"/list/{}\">{}</a>",
                html_escape(slug),
                html_escape(&row.list_name)
            ),
            _ => html_escape(&row.list_name),
        };

        let name = if row.entity_name.is_empty() {
            String::from("<span class=\"missing\">(no such entity)</span>")
        } else {
            html_escape(&row.entity_name)
        };

        html.push_str(&format!(
            "<tr class=\"{class}\"><td>{list}{privacy}</td><td>{name}</td>\
             <td><a href=\"/{slug}\">{slug}</a></td><td>{published}</td></tr>",
            class = if row.published { "" } else { "draft" },
            privacy = if row.private { " (private)" } else { "" },
            slug = html_escape(&row.entity_slug),
            published = if row.published { "yes" } else { "no" },
        ));
    }

    html.push_str("</table></body></html>");

    Ok(Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(html))
}
