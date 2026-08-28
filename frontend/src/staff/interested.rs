//! GET/POST `/staff/interested` — who has shown interest in a set of projects.
//! Ported from orrery's `interested`.
//!
//! Staff paste a list of slugs and get back everyone who joined, bookmarked or
//! reviewed any of them, which is the list they mail when a project needs
//! participants or a partner asks who their audience is.
//!
//! Interest is read from two services: joins and bookmarks live with the user
//! service, reviews with the log service. The reviewers are looked up first and
//! handed to the user service, which unions all three so a person who did all
//! three appears once.

use common::{
    message::{
        log::{ReviewersByProjectsRequest, ReviewersByProjectsResponse},
        user::{InterestedRequest, InterestedUser},
    },
    service::request,
    shared::db,
};
use poem::{Response, handler, web::Form};
use serde::Deserialize;

use super::csv_field;
use crate::component::html_escape;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SlugsForm {
    slugs: String,
}

const PAGE_STYLE: &str = "<style>body{font-family:sans-serif;margin:2rem;max-width:900px;}\
     table{border-collapse:collapse;width:100%;margin-top:1rem;}\
     td,th{padding:4px 8px;text-align:left;border-bottom:1px solid #eee;}\
     th{border-bottom:1px solid #ccc;}\
     textarea{width:100%;height:6em;}\
     .missing{color:#b00;}</style>";

/// GET `/staff/interested` — the slug form.
#[handler]
pub async fn slug_form() -> Response {
    page(&SlugsForm::default(), None)
}

/// POST `/staff/interested` — the people interested in those slugs.
#[handler]
#[tracing::instrument]
pub async fn interested(Form(form): Form<SlugsForm>) -> Result<Response, anyhow::Error> {
    // Commas or whitespace, in any mix — staff paste these out of spreadsheets
    // and chat messages.
    let slugs: Vec<String> = form
        .slugs
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();

    if slugs.is_empty() {
        return Ok(page(&form, Some(Report::default())));
    }

    // Slugs name entities; interest is recorded against the project behind the
    // entity. A slug that names no published project is reported back rather
    // than silently contributing nobody.
    let mut project_ids = Vec::new();
    let mut unknown = Vec::new();

    for slug in &slugs {
        let target = sqlx::query_scalar!(
            r#"SELECT e.target AS "target!"
               FROM entities e
               WHERE e.slug = ? AND e.kind = 'Project'"#,
            slug
        )
        .fetch_optional(db())
        .await?;

        match target {
            Some(target) => project_ids.push(target),
            None => unknown.push(slug.clone()),
        }
    }

    let reviewers: ReviewersByProjectsResponse = request(
        "log.reviewers_by_projects",
        ReviewersByProjectsRequest {
            project_ids: project_ids.clone(),
        },
    )
    .await?;

    let users: Vec<InterestedUser> = request(
        "user.interest.by_projects",
        InterestedRequest {
            project_ids,
            reviewer_ids: reviewers.user_ids,
        },
    )
    .await?;

    Ok(page(&form, Some(Report { users, unknown })))
}

#[derive(Default)]
struct Report {
    users: Vec<InterestedUser>,
    /// Slugs that matched no project.
    unknown: Vec<String>,
}

fn page(form: &SlugsForm, report: Option<Report>) -> Response {
    let mut html = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <title>Interested People</title>",
    );
    html.push_str(PAGE_STYLE);
    html.push_str(&format!(
        "</head><body><h1>Interested people</h1>\
         <p>Everyone who has joined, bookmarked or reviewed any of these projects.</p>\
         <form method=\"post\">\
         <label>Project slugs, separated by commas or spaces<br>\
         <textarea name=\"slugs\">{}</textarea></label>\
         <p><button type=\"submit\">Look up</button></p></form>",
        html_escape(&form.slugs),
    ));

    if let Some(report) = report {
        if !report.unknown.is_empty() {
            html.push_str(&format!(
                "<p class=\"missing\">No published project has {}.</p>",
                report
                    .unknown
                    .iter()
                    .map(|s| format!("<code>{}</code>", html_escape(s)))
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }

        html.push_str(&format!(
            "<p>{} {}.</p>",
            report.users.len(),
            if report.users.len() == 1 {
                "person"
            } else {
                "people"
            },
        ));

        html.push_str(
            "<table><tr><th>Name</th><th>Email</th>\
             <th>Joined</th><th>Bookmarked</th><th>Reviewed</th></tr>",
        );

        let tick = |flag: bool| if flag { "yes" } else { "" };

        for user in &report.users {
            html.push_str(&format!(
                "<tr><td>{} {}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                html_escape(&user.first_name),
                html_escape(&user.last_name),
                html_escape(&user.email),
                tick(user.joined),
                tick(user.bookmarked),
                tick(user.reviewed),
            ));
        }

        html.push_str("</table><h2>As CSV</h2><pre>");
        html.push_str("first_name,last_name,email,joined,bookmarked,reviewed\n");
        for user in &report.users {
            html.push_str(&html_escape(&format!(
                "{},{},{},{},{},{}\n",
                csv_field(&user.first_name),
                csv_field(&user.last_name),
                csv_field(&user.email),
                user.joined,
                user.bookmarked,
                user.reviewed,
            )));
        }
        html.push_str("</pre>");
    }

    html.push_str("</body></html>");

    Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(html)
}
