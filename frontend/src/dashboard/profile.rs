use std::sync::Arc;

use anyhow::Error;
use common::message::dashboard::UserProfile;
use common::message::user::{
    BasicInfo, CertificateInfo, UserBadge, UserCertificatesRequest, UserCertificatesResponse,
};
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

    // Recognition: badges and certificates. Both default to empty if their
    // service is unreachable, so the profile still renders.
    let badges: Vec<UserBadge> = common::service::request("user.badges.all", user_info.user_id)
        .await
        .unwrap_or_default();
    let certificates: UserCertificatesResponse = common::service::request(
        "user.certificates.list",
        UserCertificatesRequest {
            user_id: user_info.user_id,
        },
    )
    .await
    .unwrap_or(UserCertificatesResponse {
        certificates: vec![],
    });

    render_profile(user_info, profile, badges, certificates.certificates).await
}

async fn render_profile(
    user_info: Arc<BasicInfo>,
    profile: UserProfile,
    badges: Vec<UserBadge>,
    certificates: Vec<CertificateInfo>,
) -> Result<Response, Error> {
    let card = ProfileCard::from_user_profile(&profile);

    let badges: Vec<BadgeView> = badges
        .iter()
        .map(|b| BadgeView {
            name: b.badge.name().to_string(),
            image: b.badge.template_url().to_string(),
        })
        .collect();

    let certificates: Vec<CertView> = certificates
        .iter()
        .map(|c| CertView {
            title: crate::certificates::title_for_mode(&c.mode),
            url: format!("/-certificate/{}", c.certificate_id),
            // created_at is an ISO timestamp; show just the date.
            issued: c.created_at.split('T').next().unwrap_or("").to_string(),
        })
        .collect();

    let content = ProfileContent {
        profile,
        card,
        badges,
        certificates,
    };

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

struct BadgeView {
    name: String,
    image: String,
}

struct CertView {
    title: String,
    url: String,
    issued: String,
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
    badges: Vec<BadgeView>,
    certificates: Vec<CertView>,
}
