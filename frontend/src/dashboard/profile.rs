use std::sync::Arc;

use anyhow::Error;
use common::message::dashboard::UserProfile;
use common::message::user::BasicInfo;
use poem::{Response, handler, session::Session, web::RealIp};
use sailfish::TemplateOnce;

use crate::component::{Component, Head, StandardLayout};
use crate::middleware::authorization::Authenticated;
use crate::profile_card::ProfileCard;

#[handler]
pub async fn page(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;

    let profile: UserProfile =
        common::service::request("user.profile.get", user_info.user_id.to_string()).await?;

    render_profile(user_info, profile).await
}

async fn render_profile(
    user_info: Arc<BasicInfo>,
    profile: UserProfile,
) -> Result<Response, Error> {
    let card = ProfileCard::from_user_profile(&profile);
    let content = ProfileContent { profile, card };

    let layout = StandardLayout::new("/dashboard/profile", content)
        .prepare(Some(user_info))
        .await?;

    ProfilePage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/profile.stpl.html")]
struct ProfilePage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<ProfileContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/profile_content.stpl.html")]
struct ProfileContent {
    profile: UserProfile,
    card: ProfileCard,
}
