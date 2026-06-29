//! Staff-only reports ported from orrery's `/staff/` endpoints.

use std::collections::{HashMap, HashSet};

use common::{
    message::{
        log::{
            ParticipationByEmailRequest, ParticipationByEmailResponse, ParticipationSummaryRequest,
            ParticipationSummaryResponse,
        },
        user::WorkflowFunnelResponse,
    },
    service::request,
    shared::db,
};
use jiff::{Timestamp, ToSpan, civil, tz::TimeZone};
use poem::{Response, handler, web::Query};
use serde::Deserialize;

/// All `/staff/` endpoints, gated as a whole by `StaffOnly` (mirrors how the
/// `/-admin/` tree is protected) — so every staff route, current and future, is
/// covered by one middleware rather than per-route guards.
pub fn routes() -> impl poem::Endpoint {
    use crate::middleware::authorization::StaffOnly;
    use poem::{EndpointExt, Route, get};

    Route::new()
        .at("/affiliate-activity", get(affiliate_activity))
        .at("/published-affiliates", get(published_affiliates))
        .at("/verizon-activity", get(verizon_activity))
        .at("/site-map", get(site_map))
        .at("/training-analytics", get(training_analytics))
        .with(StaffOnly)
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[derive(Debug, Deserialize)]
pub struct AffiliateActivityQuery {
    /// Report a single day (YYYY-MM-DD).
    date: Option<String>,
    /// Otherwise, the most recent N days (default 90).
    days: Option<i64>,
}

fn csv_field(value: &str) -> String {
    if value.contains(['"', ',', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// GET `/staff/affiliate-activity` — a CSV of affiliate projects and their
/// participation activity over a window, ported from orrery. "Activity" is the
/// count of participation records in the window (each orrery `repetition` is one
/// record); "most recent participation" is the latest of all time.
#[handler]
#[tracing::instrument]
pub async fn affiliate_activity(
    Query(query): Query<AffiliateActivityQuery>,
) -> Result<Response, anyhow::Error> {
    // Reporting window [first, last): a specific `date` is that single day,
    // otherwise the most recent `days`.
    let (first_date, last_date) =
        if let Some(date) = query.date.as_deref().filter(|s| !s.is_empty()) {
            let d: civil::Date = date.parse().map_err(|e| anyhow::anyhow!("Invalid date: {e}"))?;
            (d, d.checked_add(1.day())?)
        } else {
            let days = query.days.unwrap_or(90).max(1);
            let today = Timestamp::now().to_zoned(TimeZone::UTC).date();
            (today.checked_sub(days.days())?, today)
        };
    let first = format!("{first_date}T00:00:00Z");
    let last = format!("{last_date}T00:00:00Z");

    struct AffiliateRow {
        project_id: String,
        name: String,
        status: String,
        url: String,
        flagged: bool,
        slug: Option<String>,
        publish: Option<bool>,
    }

    let affiliates = sqlx::query_as!(
        AffiliateRow,
        r#"
        SELECT
            p.project_id AS "project_id!",
            p.name AS "name!",
            p.status AS "status!",
            p.url AS "url!",
            p.flagged AS "flagged!: bool",
            e.slug AS "slug?",
            e.publish AS "publish?: bool"
        FROM projects p
        LEFT JOIN entities e ON e.target = p.project_id AND e.kind = 'Project'
        WHERE p.is_affiliate = 1
        ORDER BY p.name
        "#
    )
    .fetch_all(db())
    .await?;

    let project_ids: Vec<String> = affiliates.iter().map(|a| a.project_id.clone()).collect();
    let summary: ParticipationSummaryResponse = request(
        "log.participation_summary",
        ParticipationSummaryRequest {
            first,
            last,
            project_ids,
        },
    )
    .await?;

    let mut by_id: HashMap<String, (i64, Option<String>)> = HashMap::new();
    for entry in summary.entries {
        by_id.insert(entry.project_id, (entry.activity, entry.latest));
    }

    struct OutRow {
        name: String,
        url: String,
        project_url: String,
        status: String,
        publish: bool,
        flag: bool,
        activity: i64,
        latest: String,
    }

    let mut rows: Vec<OutRow> = affiliates
        .into_iter()
        .map(|a| {
            let (activity, latest) = by_id.get(&a.project_id).cloned().unwrap_or((0, None));
            let slug = a.slug.unwrap_or_default();
            OutRow {
                name: a.name,
                url: format!("https://scistarter.org/{slug}"),
                project_url: a.url,
                status: a.status,
                publish: a.publish.unwrap_or(false),
                flag: a.flagged,
                activity,
                // The `when` is an ISO timestamp; report just the date.
                latest: latest
                    .map(|w| w.get(0..10).unwrap_or("").to_string())
                    .unwrap_or_default(),
            }
        })
        .collect();

    rows.sort_by(|a, b| b.activity.cmp(&a.activity));

    let mut csv = String::from(
        "name,url,project url,status,publish,flag,activity,most recent participation\n",
    );
    for r in rows {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            csv_field(&r.name),
            csv_field(&r.url),
            csv_field(&r.project_url),
            csv_field(&r.status),
            if r.publish { "yes" } else { "no" },
            if r.flag { "yes" } else { "no" },
            r.activity,
            csv_field(&r.latest),
        ));
    }

    let end_label = last_date.checked_sub(1.day())?;
    let filename = format!("affiliates_activity_{first_date}_{end_label}.csv");

    Ok(poem::Response::builder()
        .content_type("text/csv")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .body(csv))
}

/// GET `/staff/published-affiliates` — CSV of published, active affiliate
/// projects (staff only). Ported from orrery's `export_published_affiliates`.
#[handler]
#[tracing::instrument]
pub async fn published_affiliates() -> Result<Response, anyhow::Error> {
    struct Row {
        project_id: String,
        name: String,
        goal: String,
        task: String,
        presenter: String,
        origin: String,
        slug: Option<String>,
    }

    let rows = sqlx::query_as!(
        Row,
        r#"
        SELECT
            p.project_id AS "project_id!",
            p.name AS "name!",
            p.goal AS "goal!",
            p.task AS "task!",
            p.presenter AS "presenter!",
            p.origin AS "origin!",
            e.slug AS "slug?"
        FROM projects p
        JOIN entities e ON e.target = p.project_id AND e.kind = 'Project'
        WHERE p.is_affiliate = 1 AND e.publish = 1 AND p.status = 'Active' AND p.origin != 'citsci'
        ORDER BY p.name
        "#
    )
    .fetch_all(db())
    .await?;

    let oneline = |s: &str| s.replace(['\n', '\r'], " ");

    let mut csv = String::from("id,title,url,goal,task,presented by,origin\n");
    for r in rows {
        let slug = r.slug.unwrap_or_default();
        csv.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            csv_field(&r.project_id),
            csv_field(&r.name),
            csv_field(&format!("https://scistarter.org/{slug}")),
            csv_field(&oneline(&r.goal)),
            csv_field(&oneline(&r.task)),
            csv_field(&r.presenter),
            csv_field(&r.origin),
        ));
    }

    let today = Timestamp::now().to_zoned(TimeZone::UTC).date();
    Ok(poem::Response::builder()
        .content_type("text/csv")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"published_affiliates_{today}.csv\""),
        )
        .body(csv))
}

/// GET `/staff/verizon-activity` — CSV of yearly contribution counts per project
/// by users with `@verizon…com` emails, since 2020 (staff only). Ported from
/// orrery's `verizon_activity`.
#[handler]
#[tracing::instrument]
pub async fn verizon_activity() -> Result<Response, anyhow::Error> {
    let summary: ParticipationByEmailResponse = request(
        "log.participation_by_email",
        ParticipationByEmailRequest {
            email_contains: "@verizon".to_string(),
            email_ends: ".com".to_string(),
            since_year: "2020".to_string(),
        },
    )
    .await?;

    // Resolve the entity ids to project names.
    let ids: Vec<String> = summary
        .entries
        .iter()
        .map(|e| e.project_id.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let mut names: HashMap<String, String> = HashMap::new();
    if !ids.is_empty() {
        use sqlx::QueryBuilder;
        let mut qb =
            QueryBuilder::new("SELECT project_id, name FROM projects WHERE project_id IN (");
        let mut sep = qb.separated(", ");
        for id in &ids {
            sep.push_bind(id);
        }
        qb.push(")");
        let rows: Vec<(String, String)> = qb.build_query_as().fetch_all(db()).await?;
        for (id, name) in rows {
            names.insert(id, name);
        }
    }

    let mut csv = String::from("year,project,contributions\n");
    for e in summary.entries {
        let project = names.get(&e.project_id).cloned().unwrap_or(e.project_id);
        csv.push_str(&format!(
            "{},{},{}\n",
            csv_field(&e.year),
            csv_field(&project),
            e.contributions,
        ));
    }

    let today = Timestamp::now().to_zoned(TimeZone::UTC).date();
    Ok(poem::Response::builder()
        .content_type("text/csv")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"verizon_activity_{today}.csv\""),
        )
        .body(csv))
}

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
