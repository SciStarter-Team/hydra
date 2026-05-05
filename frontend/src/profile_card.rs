use common::message::dashboard::{PublicProfile, PublicProfileVisibility, UserProfile};
use sailfish::TemplateOnce;
use ulid::Ulid;

/// Renders the visible, share-safe portion of a user's profile.
/// Shared between the private `/dashboard/profile` view and the
/// public `/profile/:user_id` view.
///
/// `is_self` controls whether edit chrome appears; `target_user_id`
/// is `Some` for non-self views and drives the "Send a message"
/// button. `visible` is `false` when the public view is restricted —
/// the template then renders a privacy placeholder rather than
/// profile content.
#[derive(TemplateOnce)]
#[template(path = "dashboard/_partials/profile_card.stpl.html")]
pub struct ProfileCard {
    pub handle: String,
    pub first_name: String,
    pub last_name: String,
    pub image: Option<String>,
    pub bio: Option<String>,
    pub home_page: Option<String>,
    pub facebook_url: Option<String>,
    pub twitter_profile: Option<String>,
    pub inat_username: Option<String>,
    pub zooniverse_username: Option<String>,
    pub orcid: Option<String>,
    pub scientist: bool,
    pub educator: bool,
    pub ambassador: Option<String>,
    pub date_joined: String,
    pub is_self: bool,
    pub visible: bool,
    pub target_user_id: Option<Ulid>,
}

impl ProfileCard {
    pub fn from_user_profile(profile: &UserProfile) -> Self {
        let zooniverse_username = if profile.zooniverse_username.is_empty() {
            None
        } else {
            Some(profile.zooniverse_username.clone())
        };

        ProfileCard {
            handle: profile.handle.clone(),
            first_name: profile.first_name.clone(),
            last_name: profile.last_name.clone(),
            image: profile.image.clone(),
            bio: profile.bio.clone(),
            home_page: profile.home_page.clone(),
            facebook_url: profile.facebook_url.clone(),
            twitter_profile: profile.twitter_profile.clone(),
            inat_username: profile.inat_username.clone(),
            zooniverse_username,
            orcid: profile.orcid.clone(),
            scientist: profile.scientist,
            educator: profile.educator,
            ambassador: profile.ambassador.clone(),
            date_joined: profile.date_joined.clone(),
            is_self: true,
            visible: true,
            target_user_id: None,
        }
    }

    pub fn from_public_profile(profile: &PublicProfile) -> Self {
        ProfileCard {
            handle: profile.handle.clone(),
            first_name: profile.first_name.clone(),
            last_name: profile.last_name.clone(),
            image: profile.image.clone(),
            bio: profile.bio.clone(),
            home_page: profile.home_page.clone(),
            facebook_url: profile.facebook_url.clone(),
            twitter_profile: profile.twitter_profile.clone(),
            inat_username: profile.inat_username.clone(),
            zooniverse_username: profile.zooniverse_username.clone(),
            orcid: profile.orcid.clone(),
            scientist: profile.scientist,
            educator: profile.educator,
            ambassador: profile.ambassador.clone(),
            date_joined: profile.date_joined.clone(),
            is_self: false,
            visible: profile.visibility == PublicProfileVisibility::Public,
            target_user_id: Some(profile.user_id),
        }
    }
}
