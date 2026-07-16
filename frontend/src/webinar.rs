use std::{collections::HashSet, str::FromStr};

use anyhow::Error;
use common::{
    insensitivestring::InsensitiveString, shared::db, timestring::TimeString,
    transaction::Immediate,
};
use ulid::Ulid;

use crate::peering::LoadStorePeered;

pub mod list;

/// SciStarter schedules and describes its webinars in Eastern time.
///
/// Everything that turns `held_at` into a date — the editor, the card's badge —
/// works in this zone, so an entry made as the 29th reads as the 29th
/// everywhere rather than slipping a day at some renderings and not others.
pub const WEBINAR_TIMEZONE: &str = "US/Eastern";

/// The moment a webinar was held, in the zone it was scheduled in.
///
/// `None` when it has no date, which is the un-dated draft case.
pub fn held_zoned(held_at: &TimeString) -> Option<jiff::Zoned> {
    let timestamp: jiff::Timestamp = held_at.clone().try_into().ok()?;

    Some(timestamp.to_zoned(jiff::tz::TimeZone::get(WEBINAR_TIMEZONE).ok()?))
}

/// True while a webinar dated `held_at` is still to come.
///
/// Compared by *date*, in the zone webinars are scheduled in, rather than by
/// instant. `held_at` carries only a day — session times live on the `Register`
/// links — so it is stored at midnight, and comparing instants would file a talk
/// happening at 6pm tonight under the archive from this morning onwards.
///
/// A webinar with no date is never upcoming, which is the safe reading: an entry
/// nobody has dated is not something to advertise as forthcoming.
///
/// Free-standing because the admin list draws its rows from a cut-down query
/// rather than a whole [`Webinar`], and the two must agree on what "upcoming"
/// means.
pub fn is_upcoming(held_at: &TimeString) -> bool {
    let (Some(held), Ok(zone)) = (
        held_zoned(held_at),
        jiff::tz::TimeZone::get(WEBINAR_TIMEZONE),
    ) else {
        return false;
    };

    held.date() >= jiff::Timestamp::now().to_zoned(zone).date()
}

#[derive(Debug, thiserror::Error)]
pub enum WebinarError {
    #[error("Unable to parse WebinarLinkKind from string")]
    LinkKindParse,
}

/// The kinds of resource a webinar links out to.
///
/// Which kinds are present is what separates an upcoming webinar from an
/// archived one in the rendered card: an upcoming one carries a `Register`
/// link, an archived one carries `Recording`. That is derived rather than
/// stored, so a webinar needs no explicit upcoming/archived state to fall out
/// of date.
#[derive(
    Clone, Copy, Default, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum WebinarLinkKind {
    Register,
    Recording,
    Slides,
    LinkBank,
    HostToolkit,
    #[default]
    Other,
}

impl WebinarLinkKind {
    /// Every kind staff can choose from, in the order the editor offers them.
    pub const ALL: &'static [WebinarLinkKind] = &[
        WebinarLinkKind::Register,
        WebinarLinkKind::Recording,
        WebinarLinkKind::Slides,
        WebinarLinkKind::LinkBank,
        WebinarLinkKind::HostToolkit,
        WebinarLinkKind::Other,
    ];

    /// The call to action shown when staff supply no label of their own.
    pub fn default_label(&self) -> &'static str {
        match self {
            WebinarLinkKind::Register => "Register",
            WebinarLinkKind::Recording => "Watch Recording",
            WebinarLinkKind::Slides => "Slides",
            WebinarLinkKind::LinkBank => "Link Bank",
            WebinarLinkKind::HostToolkit => "Host Toolkit",
            WebinarLinkKind::Other => "Learn More",
        }
    }
}

impl std::fmt::Display for WebinarLinkKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            WebinarLinkKind::Register => "Register",
            WebinarLinkKind::Recording => "Recording",
            WebinarLinkKind::Slides => "Slides",
            WebinarLinkKind::LinkBank => "Link Bank",
            WebinarLinkKind::HostToolkit => "Host Toolkit",
            WebinarLinkKind::Other => "Other",
        };

        text.fmt(f)
    }
}

impl FromStr for WebinarLinkKind {
    type Err = WebinarError;

    #[tracing::instrument]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Register" => Ok(WebinarLinkKind::Register),
            "Recording" => Ok(WebinarLinkKind::Recording),
            "Slides" => Ok(WebinarLinkKind::Slides),
            "Link Bank" => Ok(WebinarLinkKind::LinkBank),
            "Host Toolkit" => Ok(WebinarLinkKind::HostToolkit),
            "Other" => Ok(WebinarLinkKind::Other),
            _ => Err(WebinarError::LinkKindParse),
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct WebinarLink {
    pub kind: WebinarLinkKind,
    pub label: String,
    pub url: String,
    pub priority: i32,
}

impl WebinarLink {
    /// The text to render on the link. Per-link labels exist for the
    /// multi-session case — one webinar offering "6 PM ET" and "9 PM ET"
    /// carries two `Register` links distinguished only by their labels — so
    /// most links leave it blank and take the kind's default.
    pub fn display_label(&self) -> &str {
        if self.label.is_empty() {
            self.kind.default_label()
        } else {
            &self.label
        }
    }
}

/// A staff-authored webinar: a SciStarter production, or a partner production
/// made with direct SciStarter involvement.
///
/// Distinct from an event because the provenance differs — events are
/// user-submitted and synced from orrery, webinars are always entered by staff
/// — and because the presentation needs differ. See the `create_webinars`
/// migration.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Webinar {
    pub webinar_id: Ulid,
    pub created: TimeString,
    pub updated: TimeString,
    pub publish: bool,
    pub title: String,
    pub description: String,
    pub held_at: TimeString,
    pub platform: String,
    pub image: String,
    pub image_credit: String,
    pub featured_entity_slug: String,
    pub tags: HashSet<InsensitiveString>,
    pub links: Vec<WebinarLink>,
}

struct WebinarDB {
    webinar_id: String,
    created: TimeString,
    updated: TimeString,
    publish: bool,
    title: String,
    description: String,
    held_at: TimeString,
    platform: String,
    image: String,
    image_credit: String,
    featured_entity_slug: String,
}

impl TryFrom<WebinarDB> for Webinar {
    type Error = anyhow::Error;

    fn try_from(row: WebinarDB) -> Result<Self, Self::Error> {
        Ok(Webinar {
            webinar_id: row.webinar_id.parse()?,
            created: row.created,
            updated: row.updated,
            publish: row.publish,
            title: row.title,
            description: row.description,
            held_at: row.held_at,
            platform: row.platform,
            image: row.image,
            image_credit: row.image_credit,
            featured_entity_slug: row.featured_entity_slug,
            tags: HashSet::new(),
            links: Vec::new(),
        })
    }
}

impl Webinar {
    /// True while the webinar is still to come. See [`is_upcoming`].
    pub fn is_upcoming(&self) -> bool {
        is_upcoming(&self.held_at)
    }

    pub fn links_of_kind(&self, kind: WebinarLinkKind) -> impl Iterator<Item = &WebinarLink> {
        self.links.iter().filter(move |link| link.kind == kind)
    }

    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags
            .contains(&InsensitiveString::from(tag.to_string()))
    }

    /// Every published webinar, newest first.
    ///
    /// Tags and links are gathered in one query apiece and stitched on, rather
    /// than per webinar, so listing the whole catalog costs three queries no
    /// matter how large it grows.
    pub async fn list_published() -> Result<Vec<Webinar>, Error> {
        let rows = sqlx::query_as!(
            WebinarDB,
            r#"SELECT "webinar_id",
                      "created" as "created: TimeString",
                      "updated" as "updated: TimeString",
                      "publish" as "publish: bool",
                      "title", "description",
                      "held_at" as "held_at: TimeString",
                      "platform", "image", "image_credit", "featured_entity_slug"
               FROM "webinars"
               WHERE "publish"
               ORDER BY "held_at" DESC"#,
        )
        .fetch_all(db())
        .await?;

        let mut webinars = rows
            .into_iter()
            .map(Webinar::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        let tag_rows = sqlx::query!(
            r#"SELECT t."webinar_id", t."tag"
               FROM "webinar_tags" t
               JOIN "webinars" w ON w."webinar_id" = t."webinar_id"
               WHERE w."publish""#,
        )
        .fetch_all(db())
        .await?;

        let link_rows = sqlx::query!(
            r#"SELECT l."webinar_id", l."kind", l."label", l."url", l."priority" as "priority: i32"
               FROM "webinar_links" l
               JOIN "webinars" w ON w."webinar_id" = l."webinar_id"
               WHERE w."publish"
               ORDER BY l."priority" ASC"#,
        )
        .fetch_all(db())
        .await?;

        for webinar in webinars.iter_mut() {
            let id = webinar.webinar_id.to_string();

            webinar.tags = tag_rows
                .iter()
                .filter(|row| row.webinar_id == id)
                .map(|row| InsensitiveString::from(row.tag.clone()))
                .collect();

            webinar.links = link_rows
                .iter()
                .filter(|row| row.webinar_id == id)
                .map(|row| WebinarLink {
                    kind: row.kind.parse().unwrap_or_default(),
                    label: row.label.clone(),
                    url: row.url.clone(),
                    priority: row.priority,
                })
                .collect();
        }

        Ok(webinars)
    }
}

impl LoadStorePeered for Webinar {
    type DatabaseKey = Ulid;

    const GROUP: &str = "webinar";

    #[tracing::instrument]
    async fn read_from_database(id: Self::DatabaseKey) -> Result<Self, Error> {
        let id_str = id.to_string();

        let mut webinar: Webinar = sqlx::query_as!(
            WebinarDB,
            r#"SELECT "webinar_id",
                      "created" as "created: TimeString",
                      "updated" as "updated: TimeString",
                      "publish" as "publish: bool",
                      "title", "description",
                      "held_at" as "held_at: TimeString",
                      "platform", "image", "image_credit", "featured_entity_slug"
               FROM "webinars"
               WHERE "webinar_id" = ?"#,
            id_str,
        )
        .fetch_one(db())
        .await?
        .try_into()?;

        webinar.tags = sqlx::query!(
            r#"SELECT "tag" FROM "webinar_tags" WHERE "webinar_id" = ?"#,
            id_str,
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| InsensitiveString::from(row.tag))
        .collect();

        webinar.links = sqlx::query!(
            r#"SELECT "kind", "label", "url", "priority" as "priority: i32"
               FROM "webinar_links"
               WHERE "webinar_id" = ?
               ORDER BY "priority" ASC"#,
            id_str,
        )
        .fetch_all(db())
        .await?
        .into_iter()
        .map(|row| WebinarLink {
            kind: row.kind.parse().unwrap_or_default(),
            label: row.label,
            url: row.url,
            priority: row.priority,
        })
        .collect();

        Ok(webinar)
    }

    #[tracing::instrument]
    async fn save_to_database(&self) -> Result<(), Error> {
        let webinar_id = self.webinar_id.to_string();

        let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

        sqlx::query!(
            r#"insert or replace into "webinars" (
              "webinar_id",
              "created",
              "updated",
              "publish",
              "title",
              "description",
              "held_at",
              "platform",
              "image",
              "image_credit",
              "featured_entity_slug"
            )
            values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
            webinar_id,
            self.created,
            self.updated,
            self.publish,
            self.title,
            self.description,
            self.held_at,
            self.platform,
            self.image,
            self.image_credit,
            self.featured_entity_slug,
        )
        .execute(&mut tx)
        .await?;

        sqlx::query!(
            r#"delete from "webinar_tags" where "webinar_id" = ?"#,
            webinar_id
        )
        .execute(&mut tx)
        .await?;

        for tag in self.tags.iter() {
            let tag = tag.as_ref();

            sqlx::query!(
                r#"insert into "webinar_tags" ("webinar_id", "tag") values (?, ?)"#,
                webinar_id,
                tag,
            )
            .execute(&mut tx)
            .await?;
        }

        sqlx::query!(
            r#"delete from "webinar_links" where "webinar_id" = ?"#,
            webinar_id
        )
        .execute(&mut tx)
        .await?;

        for link in self.links.iter() {
            let kind = link.kind.to_string();

            sqlx::query!(
                r#"insert into "webinar_links" ("webinar_id", "kind", "label", "url", "priority")
                   values (?, ?, ?, ?, ?)"#,
                webinar_id,
                kind,
                link.label,
                link.url,
                link.priority,
            )
            .execute(&mut tx)
            .await?;
        }

        tx.commit().await?;

        Ok(())
    }

    #[tracing::instrument]
    async fn delete_from_database(id: Self::DatabaseKey) -> Result<(), Error> {
        let id_str = id.to_string();

        let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

        sqlx::query!(
            r#"delete from "webinar_links" where "webinar_id" = ?"#,
            id_str
        )
        .execute(&mut tx)
        .await?;

        sqlx::query!(
            r#"delete from "webinar_tags" where "webinar_id" = ?"#,
            id_str
        )
        .execute(&mut tx)
        .await?;

        sqlx::query!(r#"delete from "webinars" where "webinar_id" = ?"#, id_str)
            .execute(&mut tx)
            .await?;

        tx.commit().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    /// A webinar held `offset` days from today, in the scheduling zone.
    fn held_on(offset: i64) -> TimeString {
        let zone = jiff::tz::TimeZone::get(WEBINAR_TIMEZONE).unwrap();

        let date = jiff::Timestamp::now()
            .to_zoned(zone.clone())
            .date()
            .checked_add(jiff::Span::new().days(offset))
            .unwrap();

        TimeString::from(date.to_zoned(zone).unwrap().timestamp())
    }

    fn held(offset: i64) -> Webinar {
        Webinar {
            held_at: held_on(offset),
            ..Webinar::default()
        }
    }

    /// A talk happening tonight has not happened yet.
    ///
    /// `held_at` is stored at midnight, so comparing instants rather than dates
    /// filed today's webinar under the archive from midnight onwards — eighteen
    /// hours before it took place.
    #[test]
    fn a_webinar_held_today_is_still_upcoming() {
        check!(held(0).is_upcoming());
    }

    #[test]
    fn a_webinar_held_tomorrow_is_upcoming() {
        check!(held(1).is_upcoming());
    }

    #[test]
    fn a_webinar_held_yesterday_is_not_upcoming() {
        check!(!held(-1).is_upcoming());
    }

    #[test]
    fn an_undated_webinar_is_not_upcoming() {
        check!(!Webinar::default().is_upcoming());
    }
}
