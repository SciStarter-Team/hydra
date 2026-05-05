use std::sync::Arc;

use anyhow::Error;
use common::message::dashboard::{UserProfile, UserProfileUpdate};
use common::message::user::BasicInfo;
use poem::{
    IntoResponse, Response, handler,
    session::Session,
    web::{Form, Query, RealIp, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;

use crate::component::{Component, Head, StandardLayout};
use crate::middleware::authorization::Authenticated;

const PREFIXES: &[(InterestKind, &str)] = &[
    (InterestKind::Topic, "topic:"),
    (InterestKind::Activity, "activity:"),
    (InterestKind::Skill, "skill:"),
    (InterestKind::Tool, "tool:"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InterestKind {
    Topic,
    Activity,
    Skill,
    Tool,
}

fn prefix_for(kind: InterestKind) -> &'static str {
    PREFIXES
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, p)| *p)
        .expect("kind in PREFIXES")
}

fn split_tag_kind(tag: &str) -> Option<(InterestKind, &str)> {
    PREFIXES
        .iter()
        .find_map(|(k, p)| tag.strip_prefix(p).map(|rest| (*k, rest)))
}

fn lines_to_tags(text: &str, kind: InterestKind) -> Vec<String> {
    let prefix = prefix_for(kind);
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| format!("{}{}", prefix, line))
        .collect()
}

fn tags_to_text(tags: &[String], kind: InterestKind) -> String {
    let prefix = prefix_for(kind);
    tags.iter()
        .filter_map(|t| t.strip_prefix(prefix))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Deserialize, Default)]
pub struct InterestsQuery {
    pub saved: Option<String>,
}

#[handler]
pub async fn page(
    _session: &Session,
    RealIp(_client_ip): RealIp,
    Query(query): Query<InterestsQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;

    let profile: UserProfile =
        common::service::request("user.profile.get", user_info.user_id.to_string()).await?;

    let tags: Vec<String> =
        common::service::request("user.tags.get", user_info.user_id.to_string()).await?;

    render_interests(
        user_info,
        profile,
        tags,
        matches!(query.saved.as_deref(), Some("1")),
    )
    .await
}

#[derive(Debug, Deserialize)]
pub struct InterestsForm {
    pub topics: String,
    pub activities: String,
    pub skills: String,
    pub tools: String,
    pub extra_skills: String,
    pub extra_tools: String,
}

#[handler]
pub async fn save(
    _session: &Session,
    Authenticated(user_info): Authenticated,
    Form(form): Form<InterestsForm>,
) -> Result<Response, Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Login required"))?;
    let user_id = user_info.user_id;

    let existing: Vec<String> =
        common::service::request("user.tags.get", user_id.to_string()).await?;

    let mut merged: Vec<String> = existing
        .into_iter()
        .filter(|t| split_tag_kind(t).is_none())
        .collect();
    merged.extend(lines_to_tags(&form.topics, InterestKind::Topic));
    merged.extend(lines_to_tags(&form.activities, InterestKind::Activity));
    merged.extend(lines_to_tags(&form.skills, InterestKind::Skill));
    merged.extend(lines_to_tags(&form.tools, InterestKind::Tool));

    common::service::send("user.tags.set", (user_id.to_string(), merged)).await?;

    let update = UserProfileUpdate {
        user_id,
        handle: None,
        first_name: None,
        last_name: None,
        image: None,
        bio: None,
        birthday: None,
        gender: None,
        zipcode: None,
        country: None,
        country_code: None,
        mailing_address: None,
        facebook_url: None,
        twitter_profile: None,
        home_page: None,
        inat_username: None,
        zooniverse_username: None,
        orcid: None,
        degree: None,
        ethnicity: None,
        income: None,
        scientist: None,
        educator: None,
        public: None,
        protected: None,
        accept_emails: None,
        newsletter: None,
        accept_messages: None,
        track_contributions: None,
        ai_opt_out: None,
        extra_skills: Some(form.extra_skills),
        extra_tools: Some(form.extra_tools),
    };

    let _: () = common::service::request("user.profile.save", update).await?;

    Ok(Redirect::see_other("/dashboard/interests?saved=1").into_response())
}

async fn render_interests(
    user_info: Arc<BasicInfo>,
    profile: UserProfile,
    tags: Vec<String>,
    saved: bool,
) -> Result<Response, Error> {
    let topics = tags_to_text(&tags, InterestKind::Topic);
    let activities = tags_to_text(&tags, InterestKind::Activity);
    let skills = tags_to_text(&tags, InterestKind::Skill);
    let tools = tags_to_text(&tags, InterestKind::Tool);

    let content = InterestsContent {
        topics,
        activities,
        skills,
        tools,
        extra_skills: profile.extra_skills,
        extra_tools: profile.extra_tools,
        saved,
    };

    let layout = StandardLayout::new("/dashboard/interests", content)
        .prepare(Some(user_info))
        .await?;

    InterestsPage {
        head: Head::default().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/interests.stpl.html")]
struct InterestsPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<InterestsContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "dashboard/interests_content.stpl.html")]
struct InterestsContent {
    topics: String,
    activities: String,
    skills: String,
    tools: String,
    extra_skills: String,
    extra_tools: String,
    saved: bool,
}
