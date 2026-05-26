use std::sync::Arc;

use anyhow::Error;
use common::message::dashboard::{
    AmbassadorApplication, AmbassadorApplicationUpdate, UserProfile, UserProfileUpdate,
};
use common::message::user::BasicInfo;
use poem::{
    IntoResponse, Response, handler,
    session::Session,
    web::{Form, Query, RealIp, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::component::{Component, Head, StandardLayout};
use crate::middleware::authorization::Authenticated;

#[derive(Debug, Deserialize, Default)]
pub struct SettingsQuery {
    pub tab: Option<String>,
    pub saved: Option<String>,
}

#[handler]
pub async fn page(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Query(query): Query<SettingsQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;

    let profile: UserProfile =
        common::service::request("user.profile.get", user_info.user_id.to_string()).await?;

    let ambassador: Option<AmbassadorApplication> =
        common::service::request("user.ambassador.get", user_info.user_id.to_string()).await?;

    let mut available_tabs = vec![SettingsTab::General];
    if ambassador.is_some() {
        available_tabs.push(SettingsTab::Ambassador);
    }

    let requested_tab = match query.tab.as_deref() {
        Some("ambassador") => SettingsTab::Ambassador,
        _ => SettingsTab::General,
    };
    let active_tab = if available_tabs.contains(&requested_tab) {
        requested_tab
    } else {
        SettingsTab::General
    };
    let saved = matches!(query.saved.as_deref(), Some("1"));

    render_settings(
        user_info,
        profile,
        ambassador,
        available_tabs,
        active_tab,
        saved,
    )
    .await
}

#[derive(Debug, Deserialize)]
pub struct GeneralForm {
    pub handle: String,
    pub first_name: String,
    pub last_name: String,
    pub bio: String,
    pub birthday: String,
    pub gender: String,
    pub zipcode: String,
    pub country: String,
    pub mailing_address: String,
    pub ethnicity: String,
    pub degree: String,
    pub income: String,
    pub facebook_url: String,
    pub twitter_profile: String,
    pub home_page: String,
    pub inat_username: String,
    pub zooniverse_username: String,
    pub orcid: String,
    pub public: Option<bool>,
    pub protected: Option<bool>,
    pub track_contributions: Option<bool>,
    pub newsletter: Option<bool>,
    pub accept_messages: Option<bool>,
    pub accept_emails: Option<bool>,
    pub ai_opt_out: Option<bool>,
}

#[handler]
pub async fn save_general(
    _session: &Session,
    Authenticated(user_info): Authenticated,
    Form(form): Form<GeneralForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;

    let update = UserProfileUpdate {
        user_id: user_info.user_id,
        handle: Some(form.handle),
        first_name: Some(form.first_name),
        last_name: Some(form.last_name),
        bio: Some(form.bio),
        birthday: Some(form.birthday),
        gender: Some(form.gender),
        zipcode: Some(form.zipcode),
        country: Some(form.country),
        mailing_address: Some(form.mailing_address),
        ethnicity: Some(form.ethnicity),
        degree: Some(form.degree),
        income: Some(form.income),
        facebook_url: Some(form.facebook_url),
        twitter_profile: Some(form.twitter_profile),
        home_page: Some(form.home_page),
        inat_username: Some(form.inat_username),
        zooniverse_username: Some(form.zooniverse_username),
        orcid: Some(form.orcid),
        public: Some(form.public.unwrap_or(false)),
        protected: Some(form.protected.unwrap_or(false)),
        track_contributions: Some(form.track_contributions.unwrap_or(false)),
        newsletter: Some(form.newsletter.unwrap_or(false)),
        accept_messages: Some(form.accept_messages.unwrap_or(false)),
        accept_emails: Some(form.accept_emails.unwrap_or(false)),
        ai_opt_out: Some(form.ai_opt_out.unwrap_or(false)),
        image: None,
        country_code: None,
        scientist: None,
        educator: None,
        extra_skills: None,
        extra_tools: None,
    };

    let _: () = common::service::request("user.profile.save", update).await?;

    Ok(Redirect::see_other("/dashboard/settings?tab=general&saved=1").into_response())
}

#[derive(Debug, Deserialize)]
pub struct AmbassadorForm {
    pub bio: String,
    pub city: String,
    pub state: String,
    pub contact_email: String,
    pub youtube_video_url: String,
}

#[handler]
pub async fn save_ambassador(
    _session: &Session,
    Authenticated(user_info): Authenticated,
    Form(form): Form<AmbassadorForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;

    let update = AmbassadorApplicationUpdate {
        user_id: user_info.user_id,
        bio: form.bio,
        city: form.city,
        state: form.state,
        contact_email: form.contact_email,
        youtube_video_url: form.youtube_video_url,
    };

    let _: () = common::service::request("user.ambassador.update", update).await?;

    Ok(Redirect::see_other("/dashboard/settings?tab=ambassador&saved=1").into_response())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Ambassador,
}

async fn render_settings(
    user_info: Arc<BasicInfo>,
    profile: UserProfile,
    ambassador: Option<AmbassadorApplication>,
    available_tabs: Vec<SettingsTab>,
    active_tab: SettingsTab,
    saved: bool,
) -> Result<Response, Error> {
    let content = SettingsContent {
        available_tabs,
        active_tab,
        saved,
        profile,
        ambassador,
        user_id: user_info.user_id,
    };

    let layout = StandardLayout::new("/dashboard/settings", content)
        .prepare(Some(user_info))
        .await?;

    SettingsPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/settings.stpl.html")]
struct SettingsPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<SettingsContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/settings_content.stpl.html")]
struct SettingsContent {
    available_tabs: Vec<SettingsTab>,
    active_tab: SettingsTab,
    saved: bool,
    profile: UserProfile,
    ambassador: Option<AmbassadorApplication>,
    #[allow(dead_code)]
    user_id: Ulid,
}
