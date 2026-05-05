use std::sync::Arc;

use anyhow::Error;
use common::message::dashboard::{UserPlace, UserPlaceAdd, UserPlaceDelete};
use common::message::user::BasicInfo;
use poem::{
    IntoResponse, Response, handler,
    session::Session,
    web::{Form, Path, RealIp, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::component::{Component, Head, StandardLayout};
use crate::middleware::authorization::Authenticated;

#[handler]
pub async fn page(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;

    let places: Vec<UserPlace> =
        common::service::request("user.places.list_by_user", user_info.user_id.to_string())
            .await?;

    render_places(user_info, places).await
}

#[derive(Debug, Deserialize)]
pub struct AddPlaceForm {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

#[handler]
pub async fn add(
    _session: &Session,
    Authenticated(user_info): Authenticated,
    Form(form): Form<AddPlaceForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;

    if !form.name.trim().is_empty() {
        let request = UserPlaceAdd {
            user_id: user_info.user_id,
            name: form.name.trim().to_string(),
            latitude: form.latitude,
            longitude: form.longitude,
        };
        let _: UserPlace = common::service::request("user.places.add", request).await?;
    }

    Ok(Redirect::see_other("/dashboard/places").into_response())
}

#[handler]
pub async fn delete(
    _session: &Session,
    Authenticated(user_info): Authenticated,
    Path(place_id): Path<String>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;

    let id: Ulid = place_id
        .parse()
        .map_err(|err| anyhow::anyhow!("Invalid place id {}: {}", place_id, err))?;

    let request = UserPlaceDelete {
        user_id: user_info.user_id,
        id,
    };
    let _: () = common::service::request("user.places.delete", request).await?;

    Ok(Redirect::see_other("/dashboard/places").into_response())
}

async fn render_places(
    user_info: Arc<BasicInfo>,
    places: Vec<UserPlace>,
) -> Result<Response, Error> {
    let content = PlacesContent {
        places,
        geosearch_key: env!("GEOSEARCH_KEY").to_string(),
    };

    let layout = StandardLayout::new("/dashboard/places", content)
        .prepare(Some(user_info))
        .await?;

    PlacesPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/places.stpl.html")]
struct PlacesPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<PlacesContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/places_content.stpl.html")]
struct PlacesContent {
    places: Vec<UserPlace>,
    geosearch_key: String,
}
