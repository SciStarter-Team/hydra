//! Admin management of the choice lists (see [`crate::choice`]): adding,
//! renaming, reordering and deleting their values.
//!
//! Every change answers with the panel re-rendered, which replaces the one on
//! the page, so the list shown is always the list as it now stands.

use std::sync::Arc;

use crate::choice::{ChoiceList, ChoiceValue};
use crate::component::{AdminLayout, Component};
use crate::middleware::authorization::Authenticated;
use crate::peering::LoadStorePeered;
use common::message::user::BasicInfo;
use poem::{
    Endpoint, Route, get, handler, post,
    web::{Form, Path, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;

// ==================== Templates ====================

/// One value, with how many projects and events pick it.
struct ValueRow {
    value: ChoiceValue,
    uses: i64,
}

#[derive(TemplateOnce)]
#[template(path = "admin/choice_lists.stpl.html")]
struct ChoiceListPanel {
    list: ChoiceList,
    rows: Vec<ValueRow>,

    /// What the new-value form holds: empty, or the name just refused.
    name: String,

    /// Why the change asked for was not made.
    refusal: Option<String>,
}

impl ChoiceListPanel {
    async fn load(
        list: ChoiceList,
        name: String,
        refusal: Option<String>,
    ) -> Result<Self, anyhow::Error> {
        let usage = ChoiceValue::usage(list).await?;
        let rows = ChoiceValue::all(list)
            .await?
            .into_iter()
            .map(|value| ValueRow {
                uses: usage.get(&value.slug).copied().unwrap_or(0),
                value,
            })
            .collect();

        Ok(ChoiceListPanel {
            list,
            rows,
            name,
            refusal,
        })
    }

    async fn respond(
        list: ChoiceList,
        name: String,
        refusal: Option<String>,
    ) -> Result<poem::Response, anyhow::Error> {
        Ok(poem::Response::builder()
            .content_type("text/html")
            .body(Self::load(list, name, refusal).await?.render_once()?))
    }

    /// The panel as the list now stands, with nothing refused.
    async fn current(list: ChoiceList) -> Result<poem::Response, anyhow::Error> {
        Self::respond(list, String::new(), None).await
    }

    /// What picks a value: projects, events, or both.
    fn pickers(&self) -> &'static str {
        match self.list {
            ChoiceList::EventFormat | ChoiceList::EventProgram | ChoiceList::EventVenue => "events",
            ChoiceList::Activity => "projects",
            ChoiceList::Topic | ChoiceList::Audience => "projects and events",
        }
    }

    /// What picks a value, for just one.
    fn picker(&self) -> &'static str {
        match self.list {
            ChoiceList::EventFormat | ChoiceList::EventProgram | ChoiceList::EventVenue => "event",
            ChoiceList::Activity => "project",
            ChoiceList::Topic | ChoiceList::Audience => "project or event",
        }
    }
}

#[derive(Debug, Deserialize)]
struct ValueForm {
    #[serde(default)]
    name: String,
}

impl ValueForm {
    /// The name submitted, trimmed.
    fn name(&self) -> String {
        self.name.trim().to_string()
    }
}

/// Which way a value moves in the list.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Direction {
    Up,
    Down,
}

/// The list named by its segment of the path.
fn list_at(path: &str) -> poem::Result<ChoiceList> {
    ChoiceList::from_path(path)
        .ok_or_else(|| poem::Error::from_status(poem::http::StatusCode::NOT_FOUND))
}

// ==================== Validation ====================

/// Why `name` cannot be the name of the list's value `slug` (or of a new
/// value, when `slug` is `None`), or `None` when it can.
async fn refuse_name(
    list: ChoiceList,
    name: &str,
    slug: Option<&str>,
) -> Result<Option<String>, anyhow::Error> {
    if crate::slug::slugify(name).is_empty() {
        return Ok(Some(
            "A name needs at least one letter or digit.".to_string(),
        ));
    }

    if let Some(existing) = ChoiceValue::named(list, name).await?
        && Some(existing.slug.as_str()) != slug
    {
        return Ok(Some(format!(
            "\u{201c}{}\u{201d} is already in the list.",
            existing.name
        )));
    }

    Ok(None)
}

async fn log(
    user_info: &Option<Arc<BasicInfo>>,
    action: &str,
    extra: String,
) -> Result<(), anyhow::Error> {
    if let Some(user_info) = user_info {
        common::service::log_activity(None, action, user_info.user_id, None, extra).await?;
    }
    Ok(())
}

// ==================== Handlers ====================

#[handler]
fn first_list() -> Redirect {
    Redirect::see_other(format!(
        "/-admin/choice-lists/{}",
        ChoiceList::ALL[0].path()
    ))
}

#[handler]
async fn show(
    Path(list): Path<String>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let list = list_at(&list)?;
    let panel = ChoiceListPanel::load(list, String::new(), None).await?;

    let prepared = AdminLayout::new(list.label(), panel)
        .with_sidebar_active("choice-lists")
        .with_breadcrumb("Choice Lists", Some("/-admin/choice-lists"))
        .with_breadcrumb(list.label(), None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once().map_err(anyhow::Error::from)?))
}

#[handler]
async fn create(
    Path(list): Path<String>,
    Form(form): Form<ValueForm>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let list = list_at(&list)?;

    let name = form.name();

    if let Some(refusal) = refuse_name(list, &name, None).await? {
        return Ok(ChoiceListPanel::respond(list, name, Some(refusal)).await?);
    }

    let slug = crate::slug::slugify(&name);

    if let Some(existing) = ChoiceValue::with_slug(list, &slug).await? {
        let refusal = format!(
            "\u{201c}{name}\u{201d} is too like \u{201c}{}\u{201d}, which is already in the list.",
            existing.name
        );
        return Ok(ChoiceListPanel::respond(list, name, Some(refusal)).await?);
    }

    ChoiceValue {
        list,
        slug: slug.clone(),
        name: name.clone(),
        position: ChoiceValue::next_position(list).await?,
        orrery_id: None,
    }
    .store()
    .await?;

    log(
        &user_info,
        "choice-value-create",
        format!("{list:?} {slug} {name}"),
    )
    .await?;

    Ok(ChoiceListPanel::current(list).await?)
}

#[handler]
async fn edit(
    Path((list, slug)): Path<(String, String)>,
    Form(form): Form<ValueForm>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let list = list_at(&list)?;

    let name = form.name();

    if let Some(refusal) = refuse_name(list, &name, Some(&slug)).await? {
        return Ok(ChoiceListPanel::respond(list, String::new(), Some(refusal)).await?);
    }

    let mut value = ChoiceValue::load((list, slug.clone())).await?;
    if value.name != name {
        let was = std::mem::replace(&mut value.name, name);
        value.store().await?;
        log(
            &user_info,
            "choice-value-edit",
            format!("{list:?} {slug} {was} -> {}", value.name),
        )
        .await?;
    }

    Ok(ChoiceListPanel::current(list).await?)
}

#[handler]
async fn move_value(
    Path((list, slug, direction)): Path<(String, String, Direction)>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let list = list_at(&list)?;
    let mut values = ChoiceValue::all(list).await?;

    if let Some(index) = values.iter().position(|value| value.slug == slug) {
        let other = match direction {
            Direction::Up => index.checked_sub(1),
            Direction::Down => Some(index + 1).filter(|other| *other < values.len()),
        };

        if let Some(other) = other {
            values.swap(index, other);

            // Numbered afresh from the order shown, so ties left by earlier
            // edits are settled and the swap is all that changes.
            for (position, value) in values.iter_mut().enumerate() {
                let position = position as i64;
                if value.position != position {
                    value.position = position;
                    value.store().await?;
                }
            }

            log(
                &user_info,
                "choice-value-move",
                format!("{list:?} {slug} {direction:?}"),
            )
            .await?;
        }
    }

    Ok(ChoiceListPanel::current(list).await?)
}

#[handler]
async fn delete(
    Path((list, slug)): Path<(String, String)>,
    Authenticated(user_info): Authenticated,
) -> poem::Result<poem::Response> {
    let list = list_at(&list)?;

    ChoiceValue::delete((list, slug.clone())).await?;

    log(
        &user_info,
        "choice-value-delete",
        format!("{list:?} {slug}"),
    )
    .await?;

    Ok(ChoiceListPanel::current(list).await?)
}

// ==================== Routes ====================

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(first_list))
        .at("/:list", get(show).post(create))
        .at("/:list/:slug", poem::put(edit).delete(delete))
        .at("/:list/:slug/move/:direction", post(move_value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    /// The move route names its direction in the path.
    #[tokio::test]
    async fn a_direction_is_read_from_the_path() {
        #[handler]
        fn echo(Path((list, slug, direction)): Path<(String, String, Direction)>) -> String {
            format!("{list} {slug} {direction:?}")
        }

        let client = poem::test::TestClient::new(
            Route::new().at("/:list/:slug/move/:direction", post(echo)),
        );

        client
            .post("/event-formats/bioblitz/move/up")
            .send()
            .await
            .assert_text("event-formats bioblitz Up")
            .await;
        client
            .post("/event-formats/bioblitz/move/down")
            .send()
            .await
            .assert_text("event-formats bioblitz Down")
            .await;
        client
            .post("/event-formats/bioblitz/move/sideways")
            .send()
            .await
            .assert_status(poem::http::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn every_list_is_found_at_its_path() {
        for list in ChoiceList::ALL {
            check!(list_at(list.path()).ok() == Some(*list));
        }
        check!(list_at("sponsors").is_err());
    }

    #[test]
    fn a_name_is_read_trimmed() {
        let form = ValueForm {
            name: "  Star party ".to_string(),
        };
        check!(form.name() == "Star party");
    }

    fn row(slug: &str, name: &str, position: i64, uses: i64) -> ValueRow {
        ValueRow {
            value: ChoiceValue {
                list: ChoiceList::EventFormat,
                slug: slug.to_string(),
                name: name.to_string(),
                position,
                orrery_id: None,
            },
            uses,
        }
    }

    #[test]
    fn the_ends_of_the_list_cannot_move_past_them() {
        let html = ChoiceListPanel {
            list: ChoiceList::EventFormat,
            rows: vec![
                row("bioblitz", "BioBlitz", 0, 3),
                row("other", "Other", 1, 0),
            ],
            name: String::new(),
            refusal: None,
        }
        .render_once()
        .unwrap();

        check!(html.contains(r#"hx-post="/-admin/choice-lists/event-formats/bioblitz/move/down""#));
        check!(!html.contains(r#"hx-post="/-admin/choice-lists/event-formats/bioblitz/move/up""#));
        check!(html.contains(r#"hx-post="/-admin/choice-lists/event-formats/other/move/up""#));
        check!(!html.contains(r#"hx-post="/-admin/choice-lists/event-formats/other/move/down""#));
        check!(html.contains(r#"hx-delete="/-admin/choice-lists/event-formats/bioblitz""#));
        check!(!html.contains(r#"role="alert""#));
    }

    #[test]
    fn an_event_type_list_says_orrery_moves_values_between_them() {
        let html = |list| {
            ChoiceListPanel {
                list,
                rows: vec![],
                name: String::new(),
                refusal: None,
            }
            .render_once()
            .unwrap()
        };

        check!(html(ChoiceList::EventVenue).contains("moves here too"));
        check!(!html(ChoiceList::Topic).contains("moves here too"));
    }

    #[test]
    fn a_refused_name_stays_in_the_box_beside_the_reason() {
        let html = ChoiceListPanel {
            list: ChoiceList::EventFormat,
            rows: vec![],
            name: "<Webinar>".to_string(),
            refusal: Some("\u{201c}<Webinar>\u{201d} is already in the list.".to_string()),
        }
        .render_once()
        .unwrap();

        check!(html.contains(r#"value="&lt;Webinar&gt;""#));
        check!(html.contains(r#"role="alert""#));
        check!(!html.contains("<Webinar>"));
    }
}
