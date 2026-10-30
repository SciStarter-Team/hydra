use std::sync::Arc;

use anyhow::Error;
use common::linked_platform::LinkedPlatform;
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
use crate::dashboard::email_addresses::EmailAddressesSection;
use crate::middleware::authorization::Authenticated;
use crate::project::linked_platform::{NamedAccount, name_account};

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

    let requested_tab = match query.tab.as_deref() {
        Some("ambassador") => SettingsTab::Ambassador,
        _ => SettingsTab::General,
    };
    let notice = match query.saved.as_deref() {
        Some("1") => Some(SettingsNotice::Saved),
        _ => None,
    };

    render_settings(user_info, profile, requested_tab, notice).await
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
    pub track_contributions: Option<bool>,
    pub newsletter: Option<bool>,
    pub accept_messages: Option<bool>,
    pub accept_emails: Option<bool>,
}

impl GeneralForm {
    /// The username this form names on `platform`, as typed.
    fn platform_username(&self, platform: LinkedPlatform) -> &str {
        match platform {
            LinkedPlatform::INaturalist => &self.inat_username,
            LinkedPlatform::Zooniverse => &self.zooniverse_username,
        }
    }

    /// The save this form asks for, apart from the linked usernames, which
    /// `save_general` reads for itself.
    fn update(&self, user_id: Ulid) -> UserProfileUpdate {
        UserProfileUpdate {
            handle: Some(self.handle.clone()),
            first_name: Some(self.first_name.clone()),
            last_name: Some(self.last_name.clone()),
            bio: Some(self.bio.clone()),
            birthday: Some(self.birthday.clone()),
            gender: Some(self.gender.clone()),
            zipcode: Some(self.zipcode.clone()),
            country: Some(self.country.clone()),
            mailing_address: Some(self.mailing_address.clone()),
            ethnicity: Some(self.ethnicity.clone()),
            degree: Some(self.degree.clone()),
            income: Some(self.income.clone()),
            facebook_url: Some(self.facebook_url.clone()),
            twitter_profile: Some(self.twitter_profile.clone()),
            home_page: Some(self.home_page.clone()),
            orcid: Some(self.orcid.clone()),
            // Rendered disabled for a protected account, so it is not
            // submitted and reads as unticked here. That is the right answer
            // for them, and user.profile.save forces it either way.
            public: Some(self.public.unwrap_or(false)),
            // `protected` is not the account owner's to set. It says what
            // the site has decided about the account -- a cohort signing up
            // through a partner that enrolls minors -- and it used to ride on
            // this form as a checkbox, which meant an unticked box cleared
            // it. Left as None so the COALESCE in user.profile.save keeps
            // whatever is stored.
            track_contributions: Some(self.track_contributions.unwrap_or(false)),
            // Hidden from a protected account rather than disabled, since
            // the answer is always no; user.profile.save forces it off
            // regardless.
            newsletter: Some(self.newsletter.unwrap_or(false)),
            accept_messages: Some(self.accept_messages.unwrap_or(false)),
            accept_emails: Some(self.accept_emails.unwrap_or(false)),
            // AI recommendations are switched off, so the form no longer
            // offers the opt-out. Left as None so an earlier opt-out stays
            // recorded.
            ..UserProfileUpdate::unchanged(user_id)
        }
    }

    /// `profile` as this form would have left it, for showing the form
    /// again with what was typed when the save is refused.
    fn applied_to(&self, profile: UserProfile) -> UserProfile {
        UserProfile {
            handle: self.handle.clone(),
            first_name: self.first_name.clone(),
            last_name: self.last_name.clone(),
            bio: Some(self.bio.clone()),
            birthday: Some(self.birthday.clone()),
            gender: Some(self.gender.clone()),
            zipcode: Some(self.zipcode.clone()),
            country: Some(self.country.clone()),
            mailing_address: Some(self.mailing_address.clone()),
            ethnicity: Some(self.ethnicity.clone()),
            degree: Some(self.degree.clone()),
            income: Some(self.income.clone()),
            facebook_url: Some(self.facebook_url.clone()),
            twitter_profile: Some(self.twitter_profile.clone()),
            home_page: Some(self.home_page.clone()),
            inat_username: Some(self.inat_username.clone()),
            zooniverse_username: self.zooniverse_username.clone(),
            orcid: Some(self.orcid.clone()),
            public: self.public.unwrap_or(false),
            track_contributions: self.track_contributions.unwrap_or(false),
            newsletter: self.newsletter.unwrap_or(false),
            accept_messages: self.accept_messages.unwrap_or(false),
            accept_emails: self.accept_emails.unwrap_or(false),
            ..profile
        }
    }
}

#[handler]
pub async fn save_general(
    _session: &Session,
    Authenticated(user_info): Authenticated,
    Form(form): Form<GeneralForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;
    let user_id = user_info.user_id;

    let profile: UserProfile =
        common::service::request("user.profile.get", user_id.to_string()).await?;

    // Before anything is saved, because naming a Zooniverse account it
    // doesn't have refuses the whole save. The form posts every field back,
    // so only a name that has changed is asked about: an account already
    // naming a login Panoptes doesn't know can still save the rest of its
    // settings.
    let mut linked = Vec::new();

    for platform in LinkedPlatform::ALL {
        let typed = form.platform_username(platform);

        if platform.normalize_username(typed) == profile.platform_username(platform) {
            continue;
        }

        match name_account(platform, typed).await {
            NamedAccount::Unknown(username) => {
                let notice = SettingsNotice::UnknownAccount { platform, username };
                let shown = form.applied_to(profile);

                return render_settings(user_info, shown, SettingsTab::General, Some(notice)).await;
            }
            named => linked.push((platform, named)),
        }
    }

    let mut update = form.update(user_id);

    for (platform, named) in &linked {
        if let Some(username) = named.username() {
            update = update.with_platform_username(*platform, username.to_string());
        }
    }

    let _: () = common::service::request("user.profile.save", update).await?;

    for (_, named) in &linked {
        named.record(user_id).await;
    }

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

/// What the page says above the form about the last save.
pub enum SettingsNotice {
    Saved,
    /// Nothing was saved, because the platform has no account by the
    /// username given.
    UnknownAccount {
        platform: LinkedPlatform,
        username: String,
    },
}

async fn render_settings(
    user_info: Arc<BasicInfo>,
    profile: UserProfile,
    requested_tab: SettingsTab,
    notice: Option<SettingsNotice>,
) -> Result<Response, Error> {
    let ambassador: Option<AmbassadorApplication> =
        common::service::request("user.ambassador.get", user_info.user_id.to_string()).await?;

    let mut available_tabs = vec![SettingsTab::General];
    if ambassador.is_some() {
        available_tabs.push(SettingsTab::Ambassador);
    }

    let active_tab = if available_tabs.contains(&requested_tab) {
        requested_tab
    } else {
        SettingsTab::General
    };

    let content = SettingsContent {
        available_tabs,
        active_tab,
        notice,
        profile,
        ambassador,
        emails: EmailAddressesSection::load(user_info.user_id).await,
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
    notice: Option<SettingsNotice>,
    profile: UserProfile,
    ambassador: Option<AmbassadorApplication>,
    emails: EmailAddressesSection,
    #[allow(dead_code)]
    user_id: Ulid,
}
