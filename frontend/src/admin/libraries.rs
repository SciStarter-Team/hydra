//! Staff browser for Library Network signups.
//!
//! Mirrors orrery's `/staff/libraries` and its Django admin: browse, filter,
//! edit and export. Alongside the full export there is "Prepare Map", a narrow
//! export of just the columns a mapping tool wants, limited to signups whose
//! owner opted in.
//!
//! Note for as long as orrery still serves the sign-up form: rows that came
//! from there are re-synced from orrery, which wins on every column, so an edit
//! made here to one of those is reverted on the next pass. Rows created by our
//! own form carry no `orrery_id` and are never touched by the sync. The edit
//! page says which it is looking at.

use crate::component::{AdminLayout, Component};
use crate::form::QsQuery;
use crate::middleware::authorization::Authenticated;
use common::message::dashboard::{
    LibraryMapRow, LibrarySignupFilters, LibrarySignupListRequest, LibrarySignupListResponse,
    LibrarySignupRow, LibrarySignupUpdate, TriState,
};
use common::service::request;
use poem::{
    Endpoint, Route, get, handler,
    web::{Form, Path},
};
use sailfish::TemplateOnce;
use serde::{Deserialize, Serialize};

/// Rows per page, matching orrery's staff browser.
const PAGE_SIZE: i64 = 25;

/// Kept in step with `LibraryNetworkSignup.HAVE_ACCOUNT` in orrery.
const HAVE_ACCOUNT_CHOICES: &[(i64, &str)] = &[(0, "No"), (1, "Yes"), (2, "I don't know")];

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(index))
        .at("/export.csv", get(export_csv))
        .at("/map.csv", get(export_map_csv))
        .at("/:signup_id", get(edit).post(update))
}

/// The filter form's state, as it arrives in the query string. The
/// multi-select uses `name[]`, the only spelling `serde_qs` collects into a
/// list.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct GridQuery {
    #[serde(default)]
    pub add_to_map: TriState,
    #[serde(default)]
    pub have_account: Vec<i64>,

    #[serde(default)]
    pub recorded_from: String,
    #[serde(default)]
    pub recorded_to: String,

    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub organization_name: String,
    #[serde(default)]
    pub organization_type: String,
    #[serde(default)]
    pub primary_job_type: String,
    #[serde(default)]
    pub organization_city: String,
    #[serde(default)]
    pub organization_state: String,
    #[serde(default)]
    pub organization_zip_code: String,
    #[serde(default)]
    pub organization_country: String,
    #[serde(default)]
    pub organization_web_page: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub job_title: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<i64>,
}

impl GridQuery {
    fn page_number(&self) -> i64 {
        self.page.unwrap_or(1).max(1)
    }

    fn as_filters(&self) -> LibrarySignupFilters {
        LibrarySignupFilters {
            add_to_map: self.add_to_map,
            have_account: self.have_account.clone(),
            recorded_from: self.recorded_from.clone(),
            recorded_to: self.recorded_to.clone(),
            search: self.search.clone(),
            organization_name: self.organization_name.clone(),
            organization_type: self.organization_type.clone(),
            primary_job_type: self.primary_job_type.clone(),
            organization_city: self.organization_city.clone(),
            organization_state: self.organization_state.clone(),
            organization_zip_code: self.organization_zip_code.clone(),
            organization_country: self.organization_country.clone(),
            organization_web_page: self.organization_web_page.clone(),
            email: self.email.clone(),
            job_title: self.job_title.clone(),
        }
    }
}

#[derive(TemplateOnce)]
#[template(path = "admin/libraries.stpl.html")]
struct LibrariesPage {
    rows: Vec<LibrarySignupRow>,
    filters: GridQuery,
    have_account_choices: &'static [(i64, &'static str)],
    total: i64,
    page: i64,
    total_pages: i64,
    has_prev: bool,
    has_next: bool,
    prev_page: i64,
    next_page: i64,
    query_string: String,
}

#[handler]
async fn index(
    Authenticated(user_info): Authenticated,
    QsQuery(query): QsQuery<GridQuery>,
) -> Result<poem::Response, anyhow::Error> {
    let requested_page = query.page_number();

    let response: LibrarySignupListResponse = request(
        "user.library.list",
        LibrarySignupListRequest {
            filters: query.as_filters(),
            page: requested_page,
            per_page: PAGE_SIZE,
        },
    )
    .await?;

    let total_pages = if response.total == 0 {
        1
    } else {
        (response.total + PAGE_SIZE - 1) / PAGE_SIZE
    };
    let page_number = requested_page.min(total_pages);

    // Preserved across pagination and export links so a filtered view survives
    // both. `page` is dropped so those links can supply their own.
    let query_string = {
        let mut without_page = query.clone();
        without_page.page = None;
        serde_qs::to_string(&without_page).unwrap_or_default()
    };

    let page = LibrariesPage {
        rows: response.rows,
        filters: query,
        have_account_choices: HAVE_ACCOUNT_CHOICES,
        total: response.total,
        page: page_number,
        total_pages,
        has_prev: page_number > 1,
        has_next: page_number < total_pages,
        prev_page: (page_number - 1).max(1),
        next_page: (page_number + 1).min(total_pages),
        query_string,
    };

    let prepared = AdminLayout::new("Library Network", page)
        .with_sidebar_active("libraries")
        .with_breadcrumb("Libraries", Some("/-admin/libraries"))
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

fn csv_response(
    rows: impl IntoIterator<Item = impl Serialize>,
    filename: &str,
) -> Result<poem::Response, anyhow::Error> {
    let mut writer = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .has_headers(true)
        .from_writer(vec![]);

    for row in rows {
        writer.serialize(row)?;
    }

    Ok(poem::Response::builder()
        .content_type("text/csv")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{filename}\""),
        )
        .body(writer.into_inner()?))
}

/// Every signup matching the current filters.
#[handler]
async fn export_csv(
    Authenticated(_user_info): Authenticated,
    QsQuery(query): QsQuery<GridQuery>,
) -> Result<poem::Response, anyhow::Error> {
    let response: LibrarySignupListResponse = request(
        "user.library.list",
        LibrarySignupListRequest {
            filters: query.as_filters(),
            page: 1,
            per_page: 0,
        },
    )
    .await?;

    csv_response(response.rows, "library_signups.csv")
}

/// "Prepare Map": deliberately ignores the page's filters. It answers one
/// fixed question — which libraries asked to appear on the map — so a filter
/// left set from browsing cannot quietly shrink the map.
#[handler]
async fn export_map_csv(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let rows: Vec<LibraryMapRow> = request("user.library.map", ()).await?;

    csv_response(rows, "library_map.csv")
}

#[derive(TemplateOnce)]
#[template(path = "admin/library_edit.stpl.html")]
struct EditPage {
    row: LibrarySignupRow,
    have_account_choices: &'static [(i64, &'static str)],
    organization_types: &'static [&'static str],
    job_types: &'static [&'static str],
    saved: bool,
}

/// Kept in step with `LibraryNetworkSignup.ORGANIZATION_TYPES` in orrery.
const ORGANIZATION_TYPES: &[&str] = &[
    "Public Library",
    "State Library",
    "Academic Library",
    "Community-based Organization (ex. YMC, 4-H, Girl Scouts)",
    "School (K-12)",
    "University",
    "Museum",
    "Business",
    "Other Non-Profit",
];

/// Kept in step with `LibraryNetworkSignup.JOB_TYPES` in orrery.
const JOB_TYPES: &[&str] = &[
    "Children's/Teen Librarian",
    "Adult Services Librarian",
    "Library Assistant",
    "Library Manager/Administrator",
    "Program Director (Museum, Afterschool Program, Community-Based Organization)",
    "Program Educator (Museum, Afterschool Program, Community-Based Organization)",
    "Outreach Coordinator (Museum, Afterschool Program, Community-Based Organization)",
    "Other",
];

async fn render_edit(
    user_info: Option<std::sync::Arc<common::message::user::BasicInfo>>,
    signup_id: &str,
    saved: bool,
) -> Result<poem::Response, anyhow::Error> {
    let row: Option<LibrarySignupRow> = request("user.library.get", signup_id.to_string()).await?;
    let row = row.ok_or_else(|| anyhow::anyhow!("Library signup not found"))?;

    let page = EditPage {
        row,
        have_account_choices: HAVE_ACCOUNT_CHOICES,
        organization_types: ORGANIZATION_TYPES,
        job_types: JOB_TYPES,
        saved,
    };

    let prepared = AdminLayout::new("Edit Library Signup", page)
        .with_sidebar_active("libraries")
        .with_breadcrumb("Libraries", Some("/-admin/libraries"))
        .with_breadcrumb("Edit", None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn edit(
    Path(signup_id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    render_edit(user_info, &signup_id, false).await
}

/// The staff edit form. Checkboxes are absent when unticked, which is what
/// `Option<String>` captures.
#[derive(Debug, Deserialize)]
pub struct EditForm {
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub job_title: String,
    pub have_account: i64,
    pub organization_name: String,
    pub organization_type: String,
    pub primary_job_type: String,
    pub organization_street_address: String,
    pub organization_city: String,
    pub organization_state: String,
    pub organization_zip_code: String,
    pub organization_country: String,
    pub organization_web_page: String,
    pub organization_social_handles: String,
    #[serde(default)]
    pub add_to_map: Option<String>,
}

#[handler]
async fn update(
    Path(signup_id): Path<String>,
    Authenticated(user_info): Authenticated,
    Form(form): Form<EditForm>,
) -> Result<poem::Response, anyhow::Error> {
    let _: () = request(
        "user.library.save",
        LibrarySignupUpdate {
            signup_id: signup_id.clone(),
            user_ulid: None,
            email: form.email,
            first_name: form.first_name,
            last_name: form.last_name,
            job_title: form.job_title,
            have_account: form.have_account,
            organization_name: form.organization_name,
            organization_type: form.organization_type,
            primary_job_type: form.primary_job_type,
            organization_street_address: form.organization_street_address,
            organization_city: form.organization_city,
            organization_state: form.organization_state,
            organization_zip_code: form.organization_zip_code,
            organization_country: form.organization_country,
            organization_social_handles: form.organization_social_handles,
            organization_web_page: form.organization_web_page,
            // A staff checkbox is unambiguous, so the lenient y/yes/true rule
            // that the public form needs does not apply here.
            add_to_map: form.add_to_map.is_some(),
        },
    )
    .await?;

    render_edit(user_info, &signup_id, true).await
}
