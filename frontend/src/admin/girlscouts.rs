//! Girl Scouts Take Action submissions (top-level admin, staff-facing).
//!
//! The read side orrery never had: troops log Take Action projects at
//! `/girlscouts/action`, and this is where staff read them. Submissions are
//! records of what a troop did, so they are listed and deletable but not
//! editable --- correcting someone else's write-up would defeat the point of
//! keeping it.
//!
//! The rows come from user-service, which joins the submitter's name and email
//! in one query; see `crate::girlscouts` for why the data lives there.

use crate::component::{AdminLayout, Component};
use crate::local_time::{TimeFormat, local_time};
use crate::middleware::authorization::Authenticated;
use common::message::dashboard::{TakeActionList, TakeActionListQuery, TakeActionRow};
use common::service::request;
use poem::{
    Endpoint, Route, delete, get, handler,
    web::{Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

const PER_PAGE: i64 = 25;

#[derive(TemplateOnce)]
#[template(path = "admin/girlscouts.stpl.html")]
struct TakeActionPage {
    rows: Vec<TakeActionRow>,
    page: i64,
    pages: i64,
    total: i64,
}

#[derive(Debug, Deserialize)]
struct PageQuery {
    #[serde(default)]
    page: Option<i64>,
}

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(index))
        .at("/:submission_id", delete(remove))
}

#[handler]
async fn index(
    Authenticated(user_info): Authenticated,
    Query(query): Query<PageQuery>,
) -> Result<poem::Response, anyhow::Error> {
    // The page is clamped after the fact: the total is only known once the
    // first request comes back, so an out-of-range page asks for an empty
    // window and is then re-fetched at the last real page.
    let requested = query.page.unwrap_or(1).max(1);
    let mut list: TakeActionList = request(
        "user.girlscouts.take_action_list",
        TakeActionListQuery {
            offset: (requested - 1) * PER_PAGE,
            limit: PER_PAGE,
        },
    )
    .await?;

    let pages = ((list.total + PER_PAGE - 1) / PER_PAGE).max(1);
    let page = requested.min(pages);
    if page != requested {
        list = request(
            "user.girlscouts.take_action_list",
            TakeActionListQuery {
                offset: (page - 1) * PER_PAGE,
                limit: PER_PAGE,
            },
        )
        .await?;
    }

    let prepared = AdminLayout::new(
        "Girl Scouts Take Action",
        TakeActionPage {
            rows: list.rows,
            page,
            pages,
            total: list.total,
        },
    )
    .with_sidebar_active("girlscouts")
    .with_breadcrumb("Girl Scouts", Some("/-admin/girlscouts"))
    .prepare(user_info)
    .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn remove(Path(submission_id): Path<String>) -> Result<poem::Response, anyhow::Error> {
    let id: Ulid = submission_id.parse()?;
    request::<()>("user.girlscouts.take_action_delete", id).await?;

    // The row removes itself; the surrounding page is otherwise unchanged.
    Ok(poem::Response::builder().content_type("text/html").body(""))
}
