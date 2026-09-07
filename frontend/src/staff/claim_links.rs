//! GET/POST `/staff/claim-links` — mint the URLs that hand a page to whoever
//! signs up through them. Ported from orrery's `claim_links`.
//!
//! A partner sends us a list of pages and the people who should be running
//! them. Each link here signs its recipient up (or logs them in) and then
//! makes them the page's owner, one of its editors, and the manager and
//! contact it lists — so the person never has to be walked through claiming
//! anything, and staff never have to reassign a page by hand.
//!
//! Two steps on purpose. The first pass reports who holds each page today,
//! because a link for an already-held page takes it away from a real person,
//! and that has to be asked for rather than happen by accident.

use anyhow::Error;
use common::shared::db;
use poem::{Response, handler, web::Form};
use serde::Deserialize;
use ulid::Ulid;

use crate::component::html_escape;
use crate::form::QsForm;
use crate::invitation::{ClaimPage, Invitation, PageKind, is_unclaimed, page_owner};

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct PagesForm {
    pages: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct GenerateForm {
    pages: String,
    /// Pages to mint a link for, as `include[]` values.
    include: Vec<String>,
    /// The subset of those whose current owner may be displaced.
    transfer: Vec<String>,
}

const PAGE_STYLE: &str = "<style>body{font-family:sans-serif;margin:2rem;max-width:900px;}\
     table{border-collapse:collapse;width:100%;margin-top:1rem;}\
     td,th{padding:4px 8px;text-align:left;border-bottom:1px solid #eee;vertical-align:top;}\
     th{border-bottom:1px solid #ccc;}\
     textarea{width:100%;height:6em;}\
     tr.owned{background:#fff4e0;}\
     .missing{color:#b00;}\
     .note{color:#444;}</style>";

/// One resolved page, with what staff need to know before handing it over.
struct Page {
    page_id: Ulid,
    page_kind: PageKind,
    slug: String,
    name: String,
    owner: String,
    unclaimed: bool,
}

/// A minted link, ready to be sent to the person it is meant for.
struct Link {
    name: String,
    url: String,
    transfer: bool,
}

/// Reduce one pasted identifier to a slug or page id. Staff paste whatever
/// they have: a full page URL, a bare slug, or an id out of a report.
fn identifier_to_slug(text: &str) -> &str {
    let text = text.trim().trim_matches(',');

    // A URL, absolute or rooted: the slug is its first path segment.
    let path = match text.split_once("://") {
        Some((_, rest)) => rest.split_once('/').map(|(_, path)| path).unwrap_or(""),
        None => text.strip_prefix('/').unwrap_or(text),
    };

    path.split(['/', '?', '#']).next().unwrap_or("")
}

/// Resolve pasted identifiers to pages, in the order they were pasted.
/// Anything that names no claimable page is reported rather than dropped.
async fn resolve(pasted: &str) -> Result<(Vec<Page>, Vec<(String, String)>), Error> {
    let mut pages = Vec::new();
    let mut unresolved = Vec::new();
    let mut seen = Vec::new();

    for chunk in pasted.split(|c: char| c == ',' || c.is_whitespace()) {
        let slug = identifier_to_slug(chunk);

        if slug.is_empty() {
            continue;
        }

        // A slug names one entity and so does a page id; both columns are
        // unique, so asking about either costs one lookup.
        let entity = sqlx::query!(
            r#"SELECT e.kind AS "kind!", e.target AS "target!", e.slug AS "slug!"
               FROM entities e
               WHERE e.slug = ? OR e.target = ?"#,
            slug,
            slug
        )
        .fetch_optional(db())
        .await?;

        let Some(entity) = entity else {
            unresolved.push((slug.to_string(), "No page with that slug or id".to_string()));
            continue;
        };

        let Some(page_kind) = PageKind::from_entity_kind(&entity.kind) else {
            unresolved.push((
                slug.to_string(),
                format!("Is a {}, not a project or event", entity.kind),
            ));
            continue;
        };

        let Ok(page_id) = entity.target.parse::<Ulid>() else {
            unresolved.push((slug.to_string(), "Page has an unreadable id".to_string()));
            continue;
        };

        if seen.contains(&page_id) {
            continue;
        }

        seen.push(page_id);

        pages.push(Page {
            page_id,
            page_kind,
            slug: entity.slug,
            name: page_name(page_id, page_kind).await?,
            owner: owner_description(page_id, page_kind).await?,
            unclaimed: is_unclaimed(page_id, page_kind).await?,
        });
    }

    Ok((pages, unresolved))
}

async fn page_name(page_id: Ulid, page_kind: PageKind) -> Result<String, Error> {
    let page_id = page_id.to_string();

    Ok(match page_kind {
        PageKind::Project => {
            sqlx::query_scalar!("SELECT name FROM projects WHERE project_id = ?", page_id)
                .fetch_optional(db())
                .await?
        }
        PageKind::Event => {
            sqlx::query_scalar!("SELECT name FROM events WHERE event_id = ?", page_id)
                .fetch_optional(db())
                .await?
        }
    }
    .unwrap_or_default())
}

/// How to describe the current owner in the table: their address, or that
/// there is nobody to displace.
async fn owner_description(page_id: Ulid, page_kind: PageKind) -> Result<String, Error> {
    let Some(owner) = page_owner(page_id, page_kind).await? else {
        return Ok(String::new());
    };

    Ok(crate::admin::types::lookup_user_email(Some(owner)).await)
}

/// GET `/staff/claim-links` — the form.
#[handler]
pub async fn pages_form() -> Response {
    page(&PagesForm::default(), None, None, &[])
}

/// POST `/staff/claim-links` — report who holds each pasted page.
#[handler]
#[tracing::instrument]
pub async fn look_up(Form(form): Form<PagesForm>) -> Result<Response, Error> {
    let (pages, unresolved) = resolve(&form.pages).await?;

    Ok(page(&form, Some(&pages), None, &unresolved))
}

/// POST `/staff/claim-links/generate` — mint links for the checked pages.
#[handler]
#[tracing::instrument]
pub async fn generate(QsForm(form): QsForm<GenerateForm>) -> Result<Response, Error> {
    let (pages, mut unresolved) = resolve(&form.pages).await?;

    let wanted = |list: &[String], page_id: Ulid| list.iter().any(|id| *id == page_id.to_string());

    let mut links = Vec::new();

    for page in &pages {
        if !wanted(&form.include, page.page_id) {
            continue;
        }

        let allow_transfer = wanted(&form.transfer, page.page_id);

        if !(page.unclaimed || allow_transfer) {
            unresolved.push((
                page.slug.clone(),
                format!(
                    "Owned by {}; check the transfer box to hand it over",
                    page.owner
                ),
            ));
            continue;
        }

        let url = Invitation::ClaimPage(ClaimPage {
            page_id: page.page_id,
            page_kind: page.page_kind,
            allow_transfer,
        })
        .mint()
        .await?;

        links.push(Link {
            name: page.name.clone(),
            url,
            transfer: allow_transfer,
        });
    }

    Ok(page(
        &PagesForm {
            pages: form.pages.clone(),
        },
        None,
        Some(&links),
        &unresolved,
    ))
}

fn page(
    form: &PagesForm,
    pages: Option<&[Page]>,
    links: Option<&[Link]>,
    unresolved: &[(String, String)],
) -> Response {
    let mut html = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <title>Page Claim Links</title>",
    );
    html.push_str(PAGE_STYLE);
    html.push_str(
        "</head><body><h1>Page claim links</h1>\
         <p class=\"note\">Each link signs its recipient up (or logs them in) and then makes \
         them the owner, an editor, and the listed manager and contact of one page. Anyone who \
         opens a link gets the page, so send each one directly to the person it is meant for \
         &mdash; never post them somewhere they can be forwarded or found.</p>",
    );

    for (identifier, problem) in unresolved {
        html.push_str(&format!(
            "<p class=\"missing\"><code>{}</code>: {}</p>",
            html_escape(identifier),
            html_escape(problem),
        ));
    }

    if let Some(links) = links {
        if !links.is_empty() {
            html.push_str("<h2>Links</h2><table><tr><th>Page</th><th>Link</th></tr>");

            for link in links {
                html.push_str(&format!(
                    "<tr{}><td>{}{}</td><td><a href=\"{}\">{}</a></td></tr>",
                    if link.transfer {
                        " class=\"owned\""
                    } else {
                        ""
                    },
                    html_escape(&link.name),
                    if link.transfer {
                        "<br><em>transferred from its current owner</em>"
                    } else {
                        ""
                    },
                    html_escape(&link.url),
                    html_escape(&link.url),
                ));
            }

            html.push_str("</table><h2>Copy and paste</h2><pre>");

            for link in links {
                html.push_str(&html_escape(&format!("{}\t{}\n", link.name, link.url)));
            }

            html.push_str("</pre>");
        }

        html.push_str("<p><a href=\"/staff/claim-links\">Create more links</a></p>");
        html.push_str("</body></html>");

        return Response::builder()
            .content_type("text/html; charset=utf-8")
            .body(html);
    }

    html.push_str(&format!(
        "<form method=\"post\" action=\"/staff/claim-links\">\
         <label>Page URLs or slugs, separated by commas or spaces<br>\
         <textarea name=\"pages\">{}</textarea></label>\
         <p><button type=\"submit\">Look up</button></p></form>",
        html_escape(&form.pages),
    ));

    if let Some(pages) = pages {
        html.push_str(&format!(
            "<form method=\"post\" action=\"/staff/claim-links/generate\">\
             <input type=\"hidden\" name=\"pages\" value=\"{}\">\
             <table><tr><th>Create</th><th>Page</th><th>Type</th>\
             <th>Current owner</th><th>Transfer</th></tr>",
            html_escape(&form.pages),
        ));

        for page in pages {
            html.push_str(&format!(
                "<tr{}>\
                 <td><input type=\"checkbox\" name=\"include[]\" value=\"{id}\"{}></td>\
                 <td><a href=\"/{slug}\">{name}</a></td>\
                 <td>{kind}</td>\
                 <td>{owner}</td>\
                 <td>{transfer}</td></tr>",
                if page.unclaimed {
                    ""
                } else {
                    " class=\"owned\""
                },
                if page.unclaimed { " checked" } else { "" },
                id = page.page_id,
                slug = html_escape(&page.slug),
                name = html_escape(&page.name),
                kind = page.page_kind,
                owner = if page.unclaimed {
                    "<em>unclaimed</em>".to_string()
                } else {
                    html_escape(&page.owner)
                },
                transfer = if page.unclaimed {
                    String::new()
                } else {
                    format!(
                        "<label><input type=\"checkbox\" name=\"transfer[]\" value=\"{}\"> \
                         take it from them</label>",
                        page.page_id,
                    )
                },
            ));
        }

        html.push_str("</table><p><button type=\"submit\">Create</button></p></form>");
    }

    html.push_str("</body></html>");

    Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(html)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Staff paste whatever they have to hand: the address bar, a slug out
    /// of a spreadsheet, sometimes a link with tracking junk on the end.
    #[test]
    fn every_shape_of_identifier_reduces_to_a_slug() {
        for pasted in [
            "backyard-spectroscopy",
            "/backyard-spectroscopy",
            "https://scistarter.org/backyard-spectroscopy",
            "https://scistarter.org/backyard-spectroscopy?utm_source=mail",
            "https://scistarter.org/backyard-spectroscopy#contact",
            "  backyard-spectroscopy, ",
        ] {
            assert_eq!(identifier_to_slug(pasted), "backyard-spectroscopy");
        }
    }

    #[test]
    fn a_bare_origin_names_no_page() {
        assert_eq!(identifier_to_slug("https://scistarter.org"), "");
        assert_eq!(identifier_to_slug("https://scistarter.org/"), "");
        assert_eq!(identifier_to_slug(""), "");
    }
}
