use std::sync::Arc;

use anyhow::Error;
use common::message::dashboard::PublicProfile;
use common::message::user::{BasicInfo, UserLegacyIdLookupRequest, UserLegacyIdLookupResponse};
use poem::{
    IntoResponse, Response, handler,
    session::Session,
    web::{Path, Query, RealIp, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::component::{Component, Head, StandardLayout};
use crate::middleware::authorization::Authenticated;
use crate::profile_card::ProfileCard;

#[handler]
pub async fn public_profile_page(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Path(user_id): Path<String>,
    Authenticated(viewer_info): Authenticated,
) -> Result<Response, Error> {
    let target_id: Ulid = user_id
        .parse()
        .map_err(|err| anyhow::anyhow!("Invalid user id {}: {}", user_id, err))?;

    let viewer_is_target = viewer_info
        .as_ref()
        .map(|info| info.user_id == target_id)
        .unwrap_or(false);

    if viewer_is_target {
        return Ok(Redirect::see_other("/dashboard/profile").into_response());
    }

    let profile: PublicProfile =
        common::service::request("user.profile.public_get", target_id.to_string()).await?;

    render_public_profile(viewer_info, profile).await
}

#[derive(Debug, Deserialize)]
pub struct LegacyRedirectQuery {
    #[serde(rename = "for")]
    pub for_id: Option<String>,
}

#[handler]
pub async fn legacy_profile_redirect(
    _session: &Session,
    Query(query): Query<LegacyRedirectQuery>,
    Authenticated(viewer_info): Authenticated,
) -> Result<Response, Error> {
    if let Some(for_id) = query.for_id.as_deref() {
        if let Ok(_ulid) = for_id.parse::<Ulid>() {
            return Ok(Redirect::see_other(format!("/profile/{}", for_id)).into_response());
        }

        if let Ok(legacy_id) = for_id.parse::<i32>() {
            let response: UserLegacyIdLookupResponse = common::service::request(
                "user.legacy_id.lookup",
                UserLegacyIdLookupRequest { legacy_id },
            )
            .await?;
            if let Some(user_id) = response.user_id {
                return Ok(Redirect::see_other(format!("/profile/{}", user_id)).into_response());
            }
        }
    }

    Ok(if viewer_info.is_some() {
        Redirect::see_other("/dashboard/profile").into_response()
    } else {
        Redirect::see_other("/login").into_response()
    })
}

async fn render_public_profile(
    viewer_info: Option<Arc<BasicInfo>>,
    profile: PublicProfile,
) -> Result<Response, Error> {
    let card = ProfileCard::from_public_profile(&profile);
    let title = if !profile.handle.is_empty() {
        format!("{} - SciStarter", profile.handle)
    } else {
        "Profile - SciStarter".to_string()
    };

    let content = PublicProfileContent { card };

    let layout = StandardLayout::new("/profile", content)
        .prepare(viewer_info)
        .await?;

    PublicProfilePage {
        title,
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[derive(TemplateOnce)]
#[template(path = "profile.stpl.html")]
struct PublicProfilePage {
    title: String,
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<PublicProfileContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "profile_content.stpl.html")]
struct PublicProfileContent {
    card: ProfileCard,
}
