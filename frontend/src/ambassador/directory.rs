//! The public Ambassador directory, each ambassador's profile, and contacting
//! an ambassador through a staff-moderated message.
//!
//! Ported from orrery's `ss-ambassador-directory` and `ss-ambassador`
//! components (`get_ambassadors`, `get_ambassador`, `ambassador_send`). Who is
//! listed, and in what order, is user-service's `ambassador_directory_listing`;
//! the events a profile shows are this service's, since the event tables live
//! here.

use std::sync::Arc;

use anyhow::Error;
use common::message::dashboard::{
    AmbassadorDirectoryEntry, AmbassadorDirectoryOrder, AmbassadorDirectoryQuery,
    AmbassadorDirectoryResponse, AmbassadorMessageForm, AmbassadorProfile, AmbassadorProfileQuery,
    UserProfile,
};
use common::message::user::{
    BasicInfo, UserInfo, UserLegacyIdLookupRequest, UserLegacyIdLookupResponse,
};
use common::service::request;
use common::shared::db;
use common::timestring::TimeString;
use poem::{
    IntoResponse, Response, handler,
    http::StatusCode,
    web::{Form, Path, Query, Redirect},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use super::{DIRECTORY_PATH, profile_path};
use crate::component::{Component, Head, NumberedPagination, StandardLayout};
use crate::entity::EntityKind;
use crate::local_time::{TimeFormat, local_time};
use crate::markdown::user_content_html;
use crate::middleware::authorization::Authenticated;
use crate::middleware::error_pages::error_response;
use crate::thumbnail::thumbnail_url;

#[derive(TemplateOnce)]
#[template(path = "ambassador/page.stpl.html")]
struct DirectoryPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<DirectoryContent> as Component>::Prepared,
    title: String,
}

#[derive(TemplateOnce)]
#[template(path = "ambassador/page.stpl.html")]
struct ProfilePage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<ProfileContent> as Component>::Prepared,
    title: String,
}

/// An ambassador's card: in the directory grid, and in a profile's list of
/// nearby ambassadors.
#[derive(TemplateOnce)]
#[template(path = "ambassador/card.stpl.html")]
pub struct AmbassadorCard {
    entry: AmbassadorDirectoryEntry,
    /// The photo, sized for the card's 75px circle.
    image_src: Option<String>,
}

impl AmbassadorCard {
    async fn new(entry: AmbassadorDirectoryEntry) -> AmbassadorCard {
        let image_src = match &entry.image {
            Some(image) => Some(thumbnail_url(image, 150, 150).await),
            None => None,
        };
        AmbassadorCard { entry, image_src }
    }

    async fn all(entries: Vec<AmbassadorDirectoryEntry>) -> Vec<AmbassadorCard> {
        futures::future::join_all(entries.into_iter().map(AmbassadorCard::new)).await
    }

    fn href(&self) -> String {
        profile_path(self.entry.user_id)
    }

    /// The letter shown in place of a missing photo.
    fn initial(&self) -> char {
        self.entry.name.chars().next().unwrap_or('?')
    }
}

/// Query params for the public directory.
#[derive(Debug, Default, Deserialize)]
pub struct DirectoryParams {
    #[serde(default)]
    name: String,
    #[serde(default)]
    near: String,
    #[serde(default)]
    page: Option<i64>,
    #[serde(default)]
    order: String,
}

/// The directory body, rendered inside `StandardLayout`.
#[derive(TemplateOnce)]
#[template(path = "ambassador/directory.stpl.html")]
pub struct DirectoryContent {
    cards: Vec<AmbassadorCard>,
    name: String,
    near: String,
    total: i64,
    /// Whether a search narrowed the list, so there is something to clear.
    searching: bool,
    pagination: Option<NumberedPagination>,
}

/// The directory URL for a search, a sort and a page.
fn directory_href(name: &str, near: &str, order: AmbassadorDirectoryOrder, page: i64) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    if !name.is_empty() {
        query.append_pair("name", name);
    }
    if !near.is_empty() {
        query.append_pair("near", near);
    }
    if let Some(order) = order.as_param() {
        query.append_pair("order", order);
    }
    if page > 1 {
        query.append_pair("page", &page.to_string());
    }
    let query = query.finish();
    if query.is_empty() {
        DIRECTORY_PATH.to_string()
    } else {
        format!("{DIRECTORY_PATH}?{query}")
    }
}

#[handler]
pub async fn directory(
    Query(params): Query<DirectoryParams>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let name = params.name.trim().to_string();
    let near = params.near.trim().to_string();
    let order = AmbassadorDirectoryOrder::from_param(&params.order);

    let resp: AmbassadorDirectoryResponse = request(
        "user.ambassador.directory",
        AmbassadorDirectoryQuery {
            name: name.clone(),
            near: near.clone(),
            page: params.page.unwrap_or(1),
            order,
        },
    )
    .await?;

    let pagination = NumberedPagination::new(
        u32::try_from(resp.page).unwrap_or(1),
        u32::try_from(resp.pages).unwrap_or(0),
        |page| directory_href(&name, &near, order, page.into()),
    );

    let content = DirectoryContent {
        cards: AmbassadorCard::all(resp.entries).await,
        searching: !name.is_empty() || !near.is_empty(),
        name,
        near,
        total: resp.total,
        pagination,
    };
    let layout = StandardLayout::new(DIRECTORY_PATH, content)
        .prepare(user_info)
        .await?;
    DirectoryPage {
        head: Head::default().with_maplibre().prepare(None).await?,
        layout,
        title: "SciStarter Certified Ambassadors".to_string(),
    }
    .into_response(None)
    .await
}

/// Who is looking at a profile, as far as the page cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Viewer {
    Anonymous,
    SignedIn,
    /// The ambassador, looking at their own profile.
    Owner,
    Staff,
}

impl Viewer {
    fn of(user: &Option<Arc<BasicInfo>>, ambassador: Ulid) -> Viewer {
        match user {
            None => Viewer::Anonymous,
            Some(user) if user.user_id == ambassador => Viewer::Owner,
            Some(user) if user.can_access_admin() => Viewer::Staff,
            Some(_) => Viewer::SignedIn,
        }
    }
}

/// How the profile shows an ambassador's introduction video.
enum Video {
    /// A YouTube video, as the URL of its embeddable player.
    Embed(String),
    /// Anything else, as a link.
    Link(String),
}

impl Video {
    fn from_url(url: &str) -> Option<Video> {
        let url = url.trim();
        if url.is_empty() {
            return None;
        }
        Some(match youtube_embed_url(url) {
            Some(embed) => Video::Embed(embed),
            None => Video::Link(url.to_string()),
        })
    }
}

/// The embeddable player for a YouTube watch, share, shorts, live or embed
/// URL, or `None` if `url` is not a YouTube video.
fn youtube_embed_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed
        .host_str()?
        .trim_start_matches("www.")
        .trim_start_matches("m.");
    let mut segments = parsed.path_segments()?.filter(|s| !s.is_empty());
    let id = match host {
        "youtu.be" => segments.next()?.to_string(),
        "youtube.com" | "youtube-nocookie.com" => match segments.next()? {
            "watch" => parsed
                .query_pairs()
                .find(|(key, _)| key == "v")
                .map(|(_, value)| value.into_owned())?,
            "embed" | "shorts" | "live" | "v" => segments.next()?.to_string(),
            _ => return None,
        },
        _ => return None,
    };
    // Video ids are drawn from this alphabet; anything else is not one, and
    // must not reach the iframe's URL.
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(format!("https://www.youtube-nocookie.com/embed/{id}"))
}

/// An event on a profile.
struct HostedEvent {
    name: String,
    href: String,
    location_name: String,
    /// For an upcoming event, its next start; for a past one, its first.
    when: String,
}

impl HostedEvent {
    fn date_html(&self) -> String {
        local_time(&self.when, TimeFormat::Date)
    }
}

/// The published ambassador events an ambassador is linked to or manages,
/// split into upcoming (soonest first) and past (latest first).
///
/// orrery's rule: an event tagged with anything containing "ambassador",
/// published, and either linked to the person as an ambassador or managed by
/// them. An event with no dates at all is on neither list. The page creator
/// counts as a manager, which also covers orrery's habit of linking the owner
/// of an ambassador event automatically.
async fn hosted_events(
    user_id: Ulid,
    connected: &[Ulid],
) -> Result<(Vec<HostedEvent>, Vec<HostedEvent>), Error> {
    let user_id = user_id.to_string();
    let connected =
        serde_json::to_string(&connected.iter().map(Ulid::to_string).collect::<Vec<_>>())?;
    let now = TimeString::now().to_string();

    let rows = sqlx::query!(
        r#"SELECT e.name AS "name!", en.slug AS "slug!", e.location_name AS "location_name!",
                  (SELECT MIN(i.begin) FROM event_instances i
                    WHERE i.event_id = e.event_id AND i.begin > ?) AS "next_begin?: String",
                  (SELECT MIN(i.begin) FROM event_instances i
                    WHERE i.event_id = e.event_id) AS "first_begin?: String"
           FROM events e
           JOIN entities en ON en.target = e.event_id AND en.kind = 'Event' AND en.publish = 1
           WHERE EXISTS (SELECT 1 FROM event_tags t
                          WHERE t.event_id = e.event_id AND upper(t.tag) LIKE '%AMBASSADOR%')
             AND (e.event_id IN (SELECT value FROM json_each(?))
                  OR e.page_creator_id = ?
                  OR EXISTS (SELECT 1 FROM event_page_editors p
                              WHERE p.event_id = e.event_id AND p.user_id = ?))"#,
        now,
        connected,
        user_id,
        user_id,
    )
    .fetch_all(db())
    .await?;

    let mut upcoming = Vec::new();
    let mut past = Vec::new();
    for row in rows {
        let href = format!("{}{}", EntityKind::Event.path_prefix(), row.slug);
        let event = |when: String| HostedEvent {
            name: row.name.clone(),
            href: href.clone(),
            location_name: row.location_name.clone(),
            when,
        };
        match (row.next_begin, row.first_begin) {
            (Some(next), _) => upcoming.push(event(next)),
            (None, Some(first)) => past.push(event(first)),
            (None, None) => {}
        }
    }
    upcoming.sort_by(|a, b| a.when.cmp(&b.when));
    past.sort_by(|a, b| b.when.cmp(&a.when));
    Ok((upcoming, past))
}

/// A profile body.
#[derive(TemplateOnce)]
#[template(path = "ambassador/profile.stpl.html")]
pub struct ProfileContent {
    user_id: Ulid,
    name: String,
    first_name: String,
    /// The photo, sized for the header's 160px circle.
    image_src: Option<String>,
    city: String,
    state: String,
    bio_html: String,
    video: Option<Video>,
    listed: bool,
    viewer: Viewer,
    contact: ContactFormFragment,
    upcoming: Vec<HostedEvent>,
    past: Vec<HostedEvent>,
    nearby: Vec<AmbassadorCard>,
}

impl ProfileContent {
    fn place(&self) -> String {
        match (self.city.is_empty(), self.state.is_empty()) {
            (false, false) => format!("{}, {}", self.city, self.state),
            (false, true) => self.city.clone(),
            (true, false) => self.state.clone(),
            (true, true) => String::new(),
        }
    }

    /// Where a signed-out visitor goes to sign in and come back.
    fn login_href(&self) -> String {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair("next", &profile_path(self.user_id));
        format!("/login?{}", query.finish())
    }

    /// The staff grid, searched for this ambassador.
    fn review_href(&self) -> String {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair("search", &self.name);
        format!("/-admin/ambassadors?{}", query.finish())
    }
}

fn not_found() -> Response {
    error_response(StatusCode::NOT_FOUND, "Ambassador not found")
}

#[handler]
pub async fn profile_page(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let Ok(user_id) = Ulid::from_string(&id) else {
        return Ok(not_found());
    };
    let viewer = Viewer::of(&user_info, user_id);

    let profile: Option<AmbassadorProfile> = request(
        "user.ambassador.profile",
        AmbassadorProfileQuery {
            user_id,
            viewer: user_info.as_ref().map(|u| u.user_id),
            viewer_is_staff: viewer == Viewer::Staff,
        },
    )
    .await?;
    let Some(profile) = profile else {
        return Ok(not_found());
    };

    let (upcoming, past) = hosted_events(user_id, &profile.connected_event_ids).await?;

    // A signed-in visitor's name starts the contact form off.
    let (first_name, last_name) = match &user_info {
        Some(user) => request::<UserProfile>("user.profile.get", user.user_id.to_string())
            .await
            .map(|p| (p.first_name, p.last_name))
            .unwrap_or_default(),
        None => Default::default(),
    };

    let content = ProfileContent {
        user_id,
        contact: ContactFormFragment {
            action: contact_path(user_id),
            first_name,
            last_name,
            dates: String::new(),
            body: String::new(),
            error: None,
        },
        bio_html: user_content_html(&profile.bio),
        video: Video::from_url(&profile.youtube_video_url),
        name: profile.name,
        first_name: profile.first_name,
        image_src: match &profile.image {
            Some(image) => Some(thumbnail_url(image, 320, 320).await),
            None => None,
        },
        city: profile.city,
        state: profile.state,
        listed: profile.listed,
        viewer,
        upcoming,
        past,
        nearby: AmbassadorCard::all(profile.nearby).await,
    };
    let title = format!("{} - SciStarter Ambassador", content.name);
    let layout = StandardLayout::new(profile_path(user_id), content)
        .prepare(user_info)
        .await?;
    ProfilePage {
        head: Head::default().prepare(None).await?,
        layout,
        title,
    }
    .into_response(None)
    .await
}

/// Old profile links: orrery served profiles at `/ambassador/<its user id>`.
/// The target has to be looked up, so this is a handler rather than a
/// `Redirect` entity. A ULID is accepted too.
#[handler]
pub async fn legacy_profile_redirect(Path(id): Path<String>) -> Result<Response, Error> {
    let user_id = if let Ok(user_id) = Ulid::from_string(&id) {
        Some(user_id)
    } else if let Ok(legacy_id) = id.parse::<i32>() {
        let response: UserLegacyIdLookupResponse = request(
            "user.legacy_id.lookup",
            UserLegacyIdLookupRequest { legacy_id },
        )
        .await?;
        response.user_id
    } else {
        None
    };

    Ok(match user_id {
        Some(user_id) => Redirect::permanent(profile_path(user_id)).into_response(),
        None => not_found(),
    })
}

fn contact_path(user_id: Ulid) -> String {
    format!("{}/contact", profile_path(user_id))
}

/// The contact form, as it sits in the profile's contact dialog. A post
/// answers with the form again, carrying an error and what was typed, or with
/// [`ContactSent`] in its place.
#[derive(TemplateOnce)]
#[template(path = "ambassador/contact_form.stpl.html")]
struct ContactFormFragment {
    action: String,
    first_name: String,
    last_name: String,
    dates: String,
    body: String,
    error: Option<&'static str>,
}

/// What replaces the contact form once a message is queued.
#[derive(TemplateOnce)]
#[template(path = "ambassador/contact_sent.stpl.html")]
struct ContactSent;

/// Posted contact-an-ambassador form. Mirrors orrery's contact modal fields
/// (subject is accepted but optional; orrery has it commented out of the UI).
#[derive(Debug, Default, Deserialize)]
pub struct ContactForm {
    #[serde(default)]
    first_name: String,
    #[serde(default)]
    last_name: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    dates: String,
    #[serde(default)]
    body: String,
}

/// Queue a contact message to an ambassador (`POST <profile>/contact`). It is
/// held for staff moderation before delivery, so the visitor is told it is
/// pending review.
#[handler]
pub async fn contact(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
    Form(form): Form<ContactForm>,
) -> Result<Response, Error> {
    let Ok(recipient_id) = Ulid::from_string(&id) else {
        return Ok(not_found());
    };

    let error = if user_info.is_none() {
        Some("Please log in to contact this ambassador.")
    } else if form.first_name.trim().is_empty()
        || form.last_name.trim().is_empty()
        || form.body.trim().is_empty()
    {
        Some("Please give your name and a message.")
    } else {
        None
    };

    let (Some(sender), None) = (user_info, error) else {
        return ContactFormFragment {
            action: contact_path(recipient_id),
            first_name: form.first_name,
            last_name: form.last_name,
            dates: form.dates,
            body: form.body,
            error,
        }
        .into_response(None)
        .await;
    };

    let _: () = request(
        "user.ambassador.message_send",
        AmbassadorMessageForm {
            sender_id: sender.user_id,
            recipient_id,
            first_name: form.first_name,
            last_name: form.last_name,
            subject: form.subject,
            dates: form.dates,
            body: form.body,
        },
    )
    .await?;

    ContactSent.into_response(None).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_urls_become_embeds() {
        let embed = Some("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ".to_string());
        for url in [
            "https://youtu.be/dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ?t=42",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=x",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/shorts/dQw4w9WgXcQ",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
        ] {
            assert_eq!(youtube_embed_url(url), embed, "{url}");
        }
    }

    #[test]
    fn other_urls_are_not_youtube_embeds() {
        for url in [
            "https://vimeo.com/123",
            "https://www.youtube.com/channel/abc",
            "https://youtu.be/",
            "javascript:alert(1)",
            "https://youtu.be/abc%22onload",
            "not a url",
        ] {
            assert_eq!(youtube_embed_url(url), None, "{url}");
        }
    }

    #[test]
    fn directory_links_keep_the_search() {
        assert_eq!(
            directory_href("", "", AmbassadorDirectoryOrder::Random, 1),
            "/ambassadors-directory"
        );
        assert_eq!(
            directory_href(
                "Jane Doe",
                "Sioux Falls, SD",
                AmbassadorDirectoryOrder::Name,
                3
            ),
            "/ambassadors-directory?name=Jane+Doe&near=Sioux+Falls%2C+SD&order=name&page=3"
        );
    }
}
