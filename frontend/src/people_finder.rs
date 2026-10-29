//! The People Finder (`/people-finder`): pick an area on a map, see how many
//! people live there, and write them all one message, which staff review
//! before it is delivered. Ported from orrery's `ss-people-finder` component
//! and its `people_near_count` / `people_near_send` endpoints; how the work is
//! shared among the services is described in `common::message::people_finder`.
//!
//! Anyone may look at the counts. Sending is for SciStarter staff and for the
//! people who operate a published project or event, as in orrery, and only
//! staff may narrow an audience with the filters.
//!
//! This service's part, beyond the page, is knowing who the operators are:
//! the `published_entity_operators` view, which answers both whether the
//! sender may send and which of the people in an area are operators
//! (`frontend.people_finder.operators`). Staff review requests in
//! `admin::messages`.

use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::Error;
use async_nats::service::Request;
use common::message::people_finder::{
    MAX_RADIUS_MILES, MIN_RADIUS_MILES, PeopleFinderArea, PeopleFinderAudience,
    PeopleFinderCountRequest, PeopleFinderCounts, PeopleFinderFilter, PeopleFinderReach,
    PeopleFinderSubmission, PeopleFinderSubmitted,
};
use common::message::user::{BasicInfo, PROTECTED_SENDER_REFUSAL, UserInfo};
use common::service::request;
use common::shared::db;
use poem::{
    Response, handler,
    web::{Form, Query},
};
use sailfish::TemplateOnce;
use strum::IntoEnumIterator;
use ulid::Ulid;

use crate::component::{Component, Head, StandardLayout};
use crate::middleware::authorization::Authenticated;

/// The page.
pub const PATH: &str = "/people-finder";

/// The fragment that counts the people in the chosen area.
pub const COUNT_PATH: &str = "/people-finder/count";

/// Where the compose form posts.
pub const SEND_PATH: &str = "/people-finder/send";

/// The radius an area starts with when it is first chosen, as in orrery.
const DEFAULT_RADIUS_MILES: f64 = 50.0;

const STAFF_FILTERS_ONLY: &str = "Sorry, those filters are for SciStarter staff only.";

/// Everyone who operates a published project or event.
pub async fn operator_ids() -> Result<Vec<Ulid>, Error> {
    let rows =
        sqlx::query_scalar!(r#"SELECT "user_id" AS "user_id!" FROM "published_entity_operators""#)
            .fetch_all(db())
            .await?;

    let mut ids = Vec::with_capacity(rows.len());

    for user_id in rows {
        match Ulid::from_string(&user_id) {
            Ok(id) => ids.push(id),
            Err(err) => tracing::warn!(%user_id, "Operator id is not a ULID: {err}"),
        }
    }

    Ok(ids)
}

/// `frontend.people_finder.operators`: everyone who operates a published
/// project or event, for forum-service to deliver an approved request with.
pub async fn operators(_req: &Request) -> Result<Vec<Ulid>, Error> {
    operator_ids().await
}

async fn operates(user_id: Ulid) -> Result<bool, Error> {
    let user_id = user_id.to_string();

    Ok(sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM "published_entity_operators" WHERE "user_id" = ?
           ) AS "operates!: bool""#,
        user_id,
    )
    .fetch_one(db())
    .await?)
}

/// What the person using the page may do with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// Not signed in: may look.
    Anonymous,
    /// Signed in, but operates nothing published: may look.
    Member,
    /// Operates a published project or event: may send.
    Operator,
    /// SciStarter staff: may send, and narrow the audience with the filters.
    Staff,
}

impl Standing {
    async fn of(user: &Option<Arc<BasicInfo>>) -> Result<Standing, Error> {
        Ok(match user {
            None => Standing::Anonymous,
            Some(user) if user.is_staff() => Standing::Staff,
            Some(user) if operates(user.user_id).await? => Standing::Operator,
            Some(_) => Standing::Member,
        })
    }

    pub fn may_send(self) -> bool {
        matches!(self, Standing::Operator | Standing::Staff)
    }

    pub fn may_filter(self) -> bool {
        self == Standing::Staff
    }
}

/// Everything the page's form says. Read from the name/value pairs of the
/// form, since each filter is a checkbox of the same name.
#[derive(Debug, Default)]
struct Choice {
    area: Option<PeopleFinderArea>,
    filters: BTreeSet<PeopleFinderFilter>,
    participants: bool,
    operators: bool,
    subject: String,
    body: String,
}

impl Choice {
    fn from_pairs(pairs: Vec<(String, String)>) -> Choice {
        let mut choice = Choice::default();
        let mut latitude = None;
        let mut longitude = None;
        let mut radius_miles = DEFAULT_RADIUS_MILES;

        for (name, value) in pairs {
            match name.as_str() {
                "latitude" => latitude = value.parse::<f64>().ok(),
                "longitude" => longitude = value.parse::<f64>().ok(),
                "radius_miles" => radius_miles = value.parse().unwrap_or(DEFAULT_RADIUS_MILES),
                "filter" => {
                    if let Ok(filter) = value.parse() {
                        choice.filters.insert(filter);
                    }
                }
                "participants" => choice.participants = true,
                "operators" => choice.operators = true,
                "subject" => choice.subject = value,
                "body" => choice.body = value,
                _ => {}
            }
        }

        choice.area = match (latitude, longitude) {
            (Some(latitude), Some(longitude)) => {
                PeopleFinderArea::new(latitude, longitude, radius_miles)
            }
            _ => None,
        };

        choice
    }

    fn audience(&self) -> Option<PeopleFinderAudience> {
        Some(PeopleFinderAudience {
            area: self.area?,
            filters: self.filters.clone(),
        })
    }
}

#[derive(TemplateOnce)]
#[template(path = "people_finder/page.stpl.html")]
struct PeopleFinderPage {
    head: <Head as Component>::Prepared,
    layout: <StandardLayout<PeopleFinderContent> as Component>::Prepared,
}

/// The page body, rendered inside `StandardLayout`.
#[derive(TemplateOnce)]
#[template(path = "people_finder/content.stpl.html")]
struct PeopleFinderContent {
    standing: Standing,
    counts: CountsFragment,
    compose: ComposeSection,
}

impl PeopleFinderContent {
    /// The filters offered, which are only ever offered to staff.
    fn filters(&self) -> Vec<PeopleFinderFilter> {
        if self.standing.may_filter() {
            PeopleFinderFilter::iter().collect()
        } else {
            Vec::new()
        }
    }

    /// The radius slider's least, greatest and starting values, as its
    /// attributes spell them.
    fn radius_range(&self) -> (String, String, String) {
        let miles = |value: f64| format!("{value:.0}");
        (
            miles(MIN_RADIUS_MILES),
            miles(MAX_RADIUS_MILES),
            miles(DEFAULT_RADIUS_MILES),
        )
    }
}

/// What the count fragment shows.
#[derive(Debug, Clone, Copy)]
enum Tally {
    /// No area chosen yet.
    Unchosen,
    Counted(PeopleFinderCounts),
    Refused(&'static str),
}

/// How many people the chosen area holds, on each side of the operator
/// divide. For someone who may send, each side is a checkbox choosing whether
/// the message goes to it.
#[derive(TemplateOnce)]
#[template(path = "people_finder/counts.stpl.html")]
struct CountsFragment {
    standing: Standing,
    tally: Tally,
}

/// The compose form, or what stands in its place: a note for someone who may
/// not send, or word that a message was submitted. A post answers with this
/// section again.
#[derive(TemplateOnce)]
#[template(path = "people_finder/compose.stpl.html")]
struct ComposeSection {
    standing: Standing,
    subject: String,
    body: String,
    error: Option<&'static str>,
    submitted: bool,
}

impl ComposeSection {
    fn blank(standing: Standing) -> ComposeSection {
        ComposeSection {
            standing,
            subject: String::new(),
            body: String::new(),
            error: None,
            submitted: false,
        }
    }

    fn login_href(&self) -> String {
        format!("/login?next={PATH}")
    }
}

#[handler]
pub async fn page(Authenticated(user_info): Authenticated) -> Result<Response, Error> {
    let standing = Standing::of(&user_info).await?;

    let content = PeopleFinderContent {
        standing,
        counts: CountsFragment {
            standing,
            tally: Tally::Unchosen,
        },
        compose: ComposeSection::blank(standing),
    };
    let layout = StandardLayout::new(PATH, content)
        .prepare(user_info)
        .await?;

    PeopleFinderPage {
        head: Head::default().with_maplibre().prepare(None).await?,
        layout,
    }
    .into_response(None)
    .await
}

/// `GET /people-finder/count`: the count fragment for the area and filters
/// the page's form holds.
#[handler]
pub async fn count(
    Authenticated(user_info): Authenticated,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Result<Response, Error> {
    let standing = Standing::of(&user_info).await?;
    let choice = Choice::from_pairs(pairs);

    let tally = if !choice.filters.is_empty() && !standing.may_filter() {
        Tally::Refused(STAFF_FILTERS_ONLY)
    } else {
        match choice.audience() {
            None => Tally::Unchosen,
            Some(audience) => Tally::Counted(
                request(
                    "user.people_finder.count",
                    PeopleFinderCountRequest {
                        audience,
                        operators: operator_ids().await?,
                    },
                )
                .await?,
            ),
        }
    };

    CountsFragment { standing, tally }.into_response(None).await
}

/// `POST /people-finder/send`: submit a message for review, answering with
/// the compose section again.
#[handler]
pub async fn send(
    Authenticated(user_info): Authenticated,
    Form(pairs): Form<Vec<(String, String)>>,
) -> Result<Response, Error> {
    let standing = Standing::of(&user_info).await?;
    let choice = Choice::from_pairs(pairs);
    let reach = PeopleFinderReach::from_choices(choice.participants, choice.operators);

    let error = match (&user_info, choice.audience(), reach) {
        (None, _, _) => Some("Please sign in to send a message."),
        _ if !standing.may_send() => {
            Some("Sorry, this tool is for project and event operators only.")
        }
        (Some(sender), _, _) if !sender.may_send_messages() => Some(PROTECTED_SENDER_REFUSAL),
        _ if !choice.filters.is_empty() && !standing.may_filter() => Some(STAFF_FILTERS_ONLY),
        (_, None, _) => Some("Choose an area on the map first."),
        (_, _, None) => Some("Choose participants, operators or both."),
        _ if choice.subject.trim().is_empty() || choice.body.trim().is_empty() => {
            Some("Please give your message a subject and some text.")
        }
        _ => None,
    };

    let (Some(sender), Some(audience), Some(reach), None) =
        (user_info, choice.audience(), reach, error)
    else {
        return ComposeSection {
            standing,
            subject: choice.subject,
            body: choice.body,
            error,
            submitted: false,
        }
        .into_response(None)
        .await;
    };

    let submitted: PeopleFinderSubmitted = request(
        "forum.people_finder.submit",
        PeopleFinderSubmission {
            sender_id: sender.user_id,
            audience,
            reach,
            subject: choice.subject.trim().to_string(),
            body: choice.body,
        },
    )
    .await?;

    tracing::info!(request_id = %submitted.request_id, sender = %sender.user_id, "People Finder request submitted");

    ComposeSection {
        submitted: true,
        ..ComposeSection::blank(standing)
    }
    .into_response(None)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn the_form_reads_back_as_a_choice() {
        let choice = Choice::from_pairs(pairs(&[
            ("latitude", "40.7"),
            ("longitude", "-74.0"),
            ("radius_miles", "25"),
            ("filter", "champions"),
            ("filter", "library_network"),
            ("filter", "nonsense"),
            ("operators", "on"),
            ("subject", "Hello"),
            ("body", "Come along"),
        ]));

        let area = choice.area.unwrap();
        assert_eq!(
            (area.latitude, area.longitude, area.radius_miles),
            (40.7, -74.0, 25.0)
        );
        assert_eq!(
            choice.filters,
            [
                PeopleFinderFilter::Champions,
                PeopleFinderFilter::LibraryNetwork
            ]
            .into_iter()
            .collect()
        );
        assert!(!choice.participants);
        assert!(choice.operators);
        assert_eq!(choice.subject, "Hello");
        assert_eq!(choice.body, "Come along");
    }

    #[test]
    fn no_point_is_no_area() {
        assert!(
            Choice::from_pairs(pairs(&[("radius_miles", "25")]))
                .area
                .is_none()
        );
        assert!(
            Choice::from_pairs(pairs(&[("latitude", "40.7"), ("longitude", "")]))
                .area
                .is_none()
        );
    }

    #[test]
    fn a_missing_radius_is_the_default() {
        let choice = Choice::from_pairs(pairs(&[("latitude", "40.7"), ("longitude", "-74.0")]));
        assert_eq!(choice.area.unwrap().radius_miles, DEFAULT_RADIUS_MILES);
    }

    #[test]
    fn only_staff_may_filter_and_only_operators_and_staff_may_send() {
        assert!(Standing::Staff.may_filter());
        assert!(!Standing::Operator.may_filter());
        assert!(Standing::Operator.may_send());
        assert!(!Standing::Member.may_send());
        assert!(!Standing::Anonymous.may_send());
    }
}
