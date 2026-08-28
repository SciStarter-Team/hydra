//! Site-wide staff reports ported from orrery's `/staff/` endpoints: the
//! published-entity site map and the training engagement funnels.

use common::{message::user::WorkflowFunnelResponse, service::request, shared::db};
use poem::{Response, handler};

use crate::component::html_escape;

/// GET `/staff/site-map` — a staff listing of all published entities (name,
/// slug, kind) with links. Ported from orrery's `site_map`. Names come from the
/// project/event behind each entity (entities don't store a name), falling back
/// to the slug for other kinds.
#[handler]
#[tracing::instrument]
pub async fn site_map() -> Result<Response, anyhow::Error> {
    struct Row {
        slug: String,
        kind: String,
        name: String,
    }

    let rows = sqlx::query_as!(
        Row,
        r#"
        SELECT
            e.slug AS "slug!",
            e.kind AS "kind!",
            COALESCE(p.name, ev.name, e.slug) AS "name!"
        FROM entities e
        LEFT JOIN projects p ON e.kind = 'Project' AND p.project_id = e.target
        LEFT JOIN events ev ON e.kind = 'Event' AND ev.event_id = e.target
        WHERE e.publish = 1
        ORDER BY e.kind, e.slug
        "#
    )
    .fetch_all(db())
    .await?;

    let mut html = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Site Map</title>\
         <style>body{font-family:sans-serif;margin:2rem;}h2{margin-top:1.5rem;}\
         .row{padding:2px 0;}.slug{color:#888;font-size:.85em;margin-left:.5rem;}</style>\
         </head><body><h1>Published entities</h1>",
    );

    let mut current_kind = String::new();
    for r in rows {
        if r.kind != current_kind {
            html.push_str(&format!("<h2>{}</h2>", html_escape(&r.kind)));
            current_kind = r.kind.clone();
        }
        let slug = html_escape(&r.slug);
        html.push_str(&format!(
            "<div class=\"row\"><a href=\"/{slug}\">{name}</a><span class=\"slug\">/{slug}</span></div>",
            name = html_escape(&r.name),
        ));
    }
    html.push_str("</body></html>");

    Ok(poem::Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(html))
}

/// GET `/staff/training-analytics` — the per-workflow engagement funnel (how
/// many users reached each training step), staff only. Ported from orrery's
/// `training_analytics`. Page-view and survey-click figures from the orrery
/// report are omitted (those data sources don't map cleanly into hydra yet).
#[handler]
#[tracing::instrument]
pub async fn training_analytics() -> Result<Response, anyhow::Error> {
    let funnels: WorkflowFunnelResponse = request("user.workflow.funnel", ()).await?;

    let mut html = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Training Analytics</title>\
         <style>body{font-family:sans-serif;margin:2rem;max-width:760px;}\
         h2{margin-top:2rem;}table{border-collapse:collapse;width:100%;}\
         td,th{padding:4px 8px;text-align:left;}th{border-bottom:1px solid #ccc;}\
         .num{text-align:right;font-variant-numeric:tabular-nums;}\
         .bar{background:#037F9B;height:10px;border-radius:2px;}</style>\
         </head><body><h1>Training engagement funnels</h1>\
         <p style=\"color:#666\">Users who reached each step of each training workflow.</p>",
    );

    for funnel in &funnels.funnels {
        let started = funnel.stages.first().map(|s| s.count).unwrap_or(0);
        html.push_str(&format!("<h2>{}</h2>", html_escape(&funnel.name)));
        if let Some(note) = &funnel.note {
            html.push_str(&format!(
                "<p style=\"color:#666;font-size:.9em;margin:.25rem 0\">{}</p>",
                html_escape(note),
            ));
        }
        html.push_str(
            "<table><tr><th>Step</th><th class=\"num\">Users</th><th class=\"num\">% of started</th><th style=\"width:30%\"></th></tr>",
        );
        for s in &funnel.stages {
            let pct = if started > 0 {
                (s.count as f64) * 100.0 / (started as f64)
            } else {
                0.0
            };
            html.push_str(&format!(
                "<tr><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{:.0}%</td>\
                 <td><div class=\"bar\" style=\"width:{:.0}%\"></div></td></tr>",
                html_escape(&s.label),
                s.count,
                pct,
                pct,
            ));
        }
        html.push_str("</table>");
    }
    html.push_str("</body></html>");

    Ok(poem::Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(html))
}
