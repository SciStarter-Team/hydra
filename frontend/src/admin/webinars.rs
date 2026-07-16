use crate::component::{AdminLayout, Component};
use crate::form::QsForm;
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use crate::webinar::{WEBINAR_TIMEZONE, Webinar, WebinarLink, WebinarLinkKind, held_zoned};
use common::insensitivestring::InsensitiveString;
use common::shared::db;
use common::timestring::TimeString;
use jiff::{civil, tz::TimeZone};
use poem::{
    Endpoint, Route, get, handler,
    web::{Form, Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

// Templates
#[derive(TemplateOnce)]
#[template(path = "admin/webinars.stpl.html")]
struct WebinarsPage {
    webinars_content: String,
}

#[derive(TemplateOnce)]
#[template(path = "admin/webinars_content.stpl.html")]
struct WebinarsContentTemplate {
    webinars: Vec<WebinarListRow>,
    search_query: Option<String>,
    page: usize,
    per_page: usize,
    total_count: usize,
}

/// Just enough of a webinar to draw a row in the admin list. The full record
/// costs three queries, and the list never shows tags or links.
struct WebinarListRow {
    webinar_id: Ulid,
    title: String,
    held_at: TimeString,
    publish: bool,
}

impl WebinarListRow {
    fn held_at_display(&self) -> String {
        held_at_date(&self.held_at).unwrap_or_else(|| "—".to_string())
    }

    fn is_upcoming(&self) -> bool {
        crate::webinar::is_upcoming(&self.held_at)
    }
}

#[derive(TemplateOnce)]
#[template(path = "admin/edit_webinar_content.stpl.html")]
struct EditWebinarContent {
    webinar: Webinar,
    tags_text: String,
    held_at_value: String,
    link_kinds: &'static [WebinarLinkKind],
    error_message: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/modals/new_webinar.stpl.html")]
struct NewWebinarForm {
    error_message: Option<String>,
}

#[derive(TemplateOnce)]
#[template(path = "admin/webinar_link_row.stpl.html")]
struct WebinarLinkRow {
    link: WebinarLink,
    link_kinds: &'static [WebinarLinkKind],
}

// Query and form types
#[derive(Deserialize)]
struct SearchQuery {
    search: Option<String>,
    page: Option<usize>,
}

#[derive(Deserialize)]
struct CreateForm {
    title: String,
    held_at: Option<String>,
}

/// The edit form.
///
/// Links arrive as three parallel arrays rather than indexed maps: a row is
/// added and removed as a unit, so the arrays cannot fall out of step, and
/// there are no indices to go sparse when a row in the middle is dropped.
#[derive(Deserialize)]
struct WebinarForm {
    title: String,
    description: Option<String>,
    held_at: Option<String>,
    platform: Option<String>,
    image: Option<String>,
    image_credit: Option<String>,
    featured_entity_slug: Option<String>,
    publish: Option<String>,
    tags: Option<String>,
    #[serde(default)]
    link_kind: Vec<String>,
    #[serde(default)]
    link_label: Vec<String>,
    #[serde(default)]
    link_url: Vec<String>,
}

/// Turns a `<input type="date">` value into the stored instant.
fn parse_held_at(date: &str) -> Result<TimeString, anyhow::Error> {
    if date.trim().is_empty() {
        return Ok(TimeString::default());
    }

    let date = civil::Date::strptime("%Y-%m-%d", date.trim())
        .map_err(|_| anyhow::anyhow!("Please enter a valid date."))?;

    let zone = TimeZone::get(WEBINAR_TIMEZONE)?;

    Ok(TimeString::from(date.to_zoned(zone)?.timestamp()))
}

/// Renders the stored instant back into a `<input type="date">` value.
fn held_at_date(held_at: &TimeString) -> Option<String> {
    Some(held_zoned(held_at)?.strftime("%Y-%m-%d").to_string())
}

fn parse_tags(tags: &str) -> std::collections::HashSet<InsensitiveString> {
    tags.split(',')
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
        .map(|tag| InsensitiveString::from(tag.to_string()))
        .collect()
}

fn tags_text(webinar: &Webinar) -> String {
    let mut tags: Vec<&str> = webinar.tags.iter().map(|tag| tag.as_ref()).collect();
    tags.sort_unstable();
    tags.join(", ")
}

/// Rebuilds the link list from the submitted rows, dropping any row left blank.
fn parse_links(form: &WebinarForm) -> Result<Vec<WebinarLink>, anyhow::Error> {
    let mut links = Vec::new();

    for (index, kind) in form.link_kind.iter().enumerate() {
        let url = form.link_url.get(index).map(String::as_str).unwrap_or("");

        if url.trim().is_empty() {
            continue;
        }

        links.push(WebinarLink {
            kind: kind
                .parse::<WebinarLinkKind>()
                .map_err(|_| anyhow::anyhow!("Unrecognized link kind: {kind}"))?,
            label: form
                .link_label
                .get(index)
                .map(|label| label.trim().to_string())
                .unwrap_or_default(),
            url: url.trim().to_string(),
            priority: index as i32,
        });
    }

    Ok(links)
}

fn apply_form(webinar: &mut Webinar, form: WebinarForm) -> Result<(), anyhow::Error> {
    if form.title.trim().is_empty() {
        return Err(anyhow::anyhow!("Title is required"));
    }

    webinar.links = parse_links(&form)?;
    webinar.title = form.title.trim().to_string();
    webinar.description = form.description.unwrap_or_default();
    webinar.held_at = parse_held_at(form.held_at.as_deref().unwrap_or(""))?;
    webinar.platform = form.platform.unwrap_or_default();
    webinar.image = form.image.unwrap_or_default();
    webinar.image_credit = form.image_credit.unwrap_or_default();
    webinar.featured_entity_slug = form.featured_entity_slug.unwrap_or_default();
    webinar.publish = form.publish.is_some();
    webinar.tags = parse_tags(form.tags.as_deref().unwrap_or(""));
    webinar.updated = TimeString::now();

    Ok(())
}

// Helper functions
async fn search_impl(query: &SearchQuery) -> Result<String, anyhow::Error> {
    let page = query.page.unwrap_or(1).max(1);
    let per_page: i64 = 25;
    let offset = (page - 1) as i64 * per_page;

    let pattern = query
        .search
        .as_ref()
        .map(|term| format!("%{term}%"))
        .unwrap_or_else(|| "%".to_string());

    let rows = sqlx::query!(
        r#"SELECT "webinar_id",
                  "title",
                  "held_at" as "held_at: TimeString",
                  "publish" as "publish: bool"
           FROM "webinars"
           WHERE "title" LIKE ?1
           ORDER BY "held_at" DESC
           LIMIT ?2 OFFSET ?3"#,
        pattern,
        per_page,
        offset
    )
    .fetch_all(db())
    .await?;

    let webinars = rows
        .into_iter()
        .map(|row| {
            Ok(WebinarListRow {
                webinar_id: row.webinar_id.parse()?,
                title: row.title,
                held_at: row.held_at,
                publish: row.publish,
            })
        })
        .collect::<Result<Vec<_>, anyhow::Error>>()?;

    let total_count: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*) FROM "webinars" WHERE "title" LIKE ?"#,
        pattern
    )
    .fetch_one(db())
    .await?;

    let template = WebinarsContentTemplate {
        webinars,
        search_query: query.search.clone(),
        page,
        per_page: per_page as usize,
        total_count: total_count as usize,
    };

    Ok(template.render_once()?)
}

async fn edit_page(
    webinar: Webinar,
    error_message: Option<String>,
    user_info: Option<std::sync::Arc<common::message::user::BasicInfo>>,
) -> Result<poem::Response, anyhow::Error> {
    let content = EditWebinarContent {
        tags_text: tags_text(&webinar),
        held_at_value: held_at_date(&webinar.held_at).unwrap_or_default(),
        link_kinds: WebinarLinkKind::ALL,
        webinar,
        error_message,
    };

    let prepared = AdminLayout::new("Edit Webinar", content)
        .with_breadcrumb("Webinars", Some("/-admin/webinars"))
        .with_breadcrumb("Edit", None)
        .with_sidebar_active("webinars")
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

// Handlers
#[handler]
async fn list(Authenticated(user_info): Authenticated) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("User not authenticated"))?;

    let search_results = search_impl(&SearchQuery {
        search: None,
        page: Some(1),
    })
    .await?;

    let page = WebinarsPage {
        webinars_content: format!(r#"<div id="webinars-content">{search_results}</div>"#),
    };

    let prepared = AdminLayout::new("Webinars", page)
        .with_sidebar_active("webinars")
        .with_breadcrumb("Webinars", None)
        .prepare(Some(user_info))
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn search(
    Query(query): Query<SearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(search_impl(&query).await?))
}

#[handler]
async fn new_form(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let form = NewWebinarForm {
        error_message: None,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(form.render_once()?))
}

/// Creates the record and hands straight off to the editor.
///
/// The modal asks only for a title and date so that the full field set — tags,
/// links, imagery — lives in exactly one template rather than being duplicated
/// between a create form and an edit form.
#[handler]
async fn create(
    Form(form): Form<CreateForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    match (async || -> Result<Ulid, anyhow::Error> {
        if form.title.trim().is_empty() {
            return Err(anyhow::anyhow!("Title is required"));
        }

        let webinar = Webinar {
            webinar_id: Ulid::new(),
            created: TimeString::now(),
            updated: TimeString::now(),
            publish: false,
            title: form.title.trim().to_string(),
            held_at: parse_held_at(form.held_at.as_deref().unwrap_or(""))?,
            ..Webinar::default()
        };

        let id = webinar.webinar_id;
        webinar.store().await?;

        if let Some(user_info) = &user_info {
            common::service::log_activity(
                None,
                "webinar-create",
                user_info.user_id,
                id,
                format!("title={}", form.title.trim()),
            )
            .await?;
        }

        Ok(id)
    })()
    .await
    {
        Ok(id) => Ok(poem::Response::builder()
            .content_type("text/html")
            .header("HX-Redirect", format!("/-admin/webinars/{id}"))
            .body("")),
        Err(e) => {
            let form = NewWebinarForm {
                error_message: Some(e.to_string()),
            };

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(form.render_once()?))
        }
    }
}

#[handler]
async fn edit(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let webinar = Webinar::load(id_str.parse()?).await?;

    edit_page(webinar, None, user_info).await
}

#[handler]
async fn update(
    Path(id_str): Path<String>,
    QsForm(form): QsForm<WebinarForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let webinar_id: Ulid = id_str.parse()?;
    let mut webinar = Webinar::load(webinar_id).await?;

    match apply_form(&mut webinar, form) {
        Ok(()) => {
            webinar.store().await?;

            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "webinar-update",
                    user_info.user_id,
                    webinar_id,
                    format!("title={}", webinar.title),
                )
                .await?;
            }

            Ok(poem::Response::builder()
                .status(poem::http::StatusCode::SEE_OTHER)
                .header("Location", "/-admin/webinars")
                .finish())
        }
        Err(e) => {
            // Redraw what was submitted, not what is stored, so the editor does
            // not silently discard the rest of the changes on one bad field.
            edit_page(webinar, Some(e.to_string()), user_info).await
        }
    }
}

#[handler]
async fn delete(
    Path(id_str): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let webinar_id: Ulid = id_str.parse()?;

    Webinar::delete(webinar_id).await?;

    if let Some(user_info) = &user_info {
        common::service::log_activity(None, "webinar-delete", user_info.user_id, webinar_id, None)
            .await?;
    }

    Ok(poem::Response::builder().content_type("text/html").body(""))
}

/// Serves one blank link row, so the editor grows without client-side markup.
#[handler]
async fn link_row(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let row = WebinarLinkRow {
        link: WebinarLink::default(),
        link_kinds: WebinarLinkKind::ALL,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(row.render_once()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    fn parse_form(query: &str) -> WebinarForm {
        serde_qs::Config::new(10, false)
            .deserialize_str::<WebinarForm>(query)
            .unwrap()
    }

    /// A date entered in the editor has to come back as the same date, or staff
    /// see their entry shift by a day every time they reopen the record.
    #[test]
    fn held_at_survives_a_round_trip_through_the_editor() {
        let stored = parse_held_at("2026-04-29").unwrap();

        check!(held_at_date(&stored).unwrap() == "2026-04-29");
    }

    /// Midnight Eastern is late evening UTC the day before, so a webinar stored
    /// naively as UTC midnight would render as the 28th.
    #[test]
    fn held_at_is_anchored_in_eastern_time() {
        let stored = parse_held_at("2026-04-29").unwrap();

        check!(stored.to_string().starts_with("2026-04-29T04:00:00"));
    }

    #[test]
    fn a_webinar_with_no_date_is_not_upcoming() {
        let webinar = Webinar {
            held_at: parse_held_at("").unwrap(),
            ..Webinar::default()
        };

        check!(!webinar.is_upcoming());
    }

    #[test]
    fn tags_are_split_and_trimmed_and_blanks_dropped() {
        let tags = parse_tags("For the Public, Pollinators ,, Teens ");

        check!(tags.len() == 3);
        check!(tags.contains(&InsensitiveString::from("Pollinators".to_string())));
        check!(tags.contains(&InsensitiveString::from("For the Public".to_string())));
    }

    /// The three link arrays are only trustworthy if they arrive aligned, which
    /// is the whole reason rows are added and removed as a unit.
    #[test]
    fn link_rows_arrive_as_aligned_arrays() {
        let form = parse_form(
            "title=Anything\
             &link_kind[]=Register&link_kind[]=Register\
             &link_label[]=6 PM ET&link_label[]=9 PM ET\
             &link_url[]=https://example.com/six&link_url[]=https://example.com/nine",
        );

        let links = parse_links(&form).unwrap();

        check!(links.len() == 2);
        check!(links[0].kind == WebinarLinkKind::Register);
        check!(links[0].label == "6 PM ET");
        check!(links[1].label == "9 PM ET");
        // Priority follows submitted order, so staff reorder by moving rows.
        check!(links[0].priority == 0);
        check!(links[1].priority == 1);
    }

    /// A row the editor added but never filled in should evaporate rather than
    /// persist as a link to nowhere.
    #[test]
    fn link_rows_without_an_address_are_dropped() {
        let form = parse_form(
            "title=Anything\
             &link_kind[]=Recording&link_kind[]=Slides\
             &link_label[]=&link_label[]=\
             &link_url[]=https://example.com/watch&link_url[]=",
        );

        let links = parse_links(&form).unwrap();

        check!(links.len() == 1);
        check!(links[0].kind == WebinarLinkKind::Recording);
    }

    /// The label the reader sees falls back to the kind when staff leave it
    /// blank, which is the common single-session case.
    #[test]
    fn a_link_without_a_label_shows_its_kinds_call_to_action() {
        let link = WebinarLink {
            kind: WebinarLinkKind::Recording,
            ..WebinarLink::default()
        };

        check!(link.display_label() == "Watch Recording");
    }

    #[test]
    fn an_unpublished_webinar_stays_a_draft_unless_the_box_is_ticked() {
        let mut webinar = Webinar::default();

        apply_form(&mut webinar, parse_form("title=Anything")).unwrap();
        check!(!webinar.publish);

        apply_form(&mut webinar, parse_form("title=Anything&publish=true")).unwrap();
        check!(webinar.publish);
    }

    #[test]
    fn a_webinar_must_have_a_title() {
        let mut webinar = Webinar::default();

        check!(apply_form(&mut webinar, parse_form("title=   ")).is_err());
    }
}

// Routes export
pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(list).post(create))
        .at("/search", get(search))
        .at("/new", get(new_form))
        .at("/link-row", get(link_row))
        .at("/:id", get(edit).post(update).delete(delete))
}
