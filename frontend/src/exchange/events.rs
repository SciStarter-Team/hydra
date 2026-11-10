//! An exchange's events: the listing shown above its projects, with its own
//! pager. The page for each event is [`super::handlers::exchange_event`].
//!
//! Which events an exchange lists, and in what order, is orrery's
//! `Exchange::find_events` (src/common.rs), and the two must agree so an
//! exchange lists the same events on either side of the cutover:
//!
//! - membership, the region and the "include global" box, the region
//!   tick-boxes and the squelches are the ones its projects are narrowed by;
//! - an event is listed while it has a session that has not ended, or
//!   always once past events are asked for;
//! - activities, topics and audiences belong to projects alone, so a search
//!   narrowed by any of them lists no events;
//! - soonest first, then the most recently ended, then those that never
//!   gave their dates.

use std::collections::HashMap;

use anyhow::Error;
use poem::{
    IntoResponse, handler,
    web::{Html, Path},
};
use sailfish::TemplateOnce;

use super::handlers::{
    EVENTS, PageSelector, SearchScope, create_template_exchange, push_hidden_exclusion,
    push_membership, push_named_regions,
};
use super::{ExchangeInfo, SearchParams, TemplateExchange};
use crate::local_time::{TimeFormat, local_time, local_time_range};
use crate::middleware::authorization::Authenticated;
use crate::project::location::Where;

/// How many events a page of an exchange's listing holds: one row of cards.
pub const PAGE_SIZE: usize = 4;

/// One session of an event, as `event_instances` holds it.
#[derive(Debug, Clone)]
pub struct Session {
    pub begin: String,
    pub end: String,
}

impl Session {
    /// Whether it ended before `now`, a time as `TimeString` writes it.
    pub fn is_over(&self, now: &str) -> bool {
        self.end.as_str() < now
    }

    /// Whether it runs a day or more, and so reads better as a span of dates
    /// than as the time it begins.
    pub fn spans_days(&self) -> bool {
        match (
            self.begin.parse::<jiff::Timestamp>(),
            self.end.parse::<jiff::Timestamp>(),
        ) {
            (Ok(begin), Ok(end)) => {
                end.duration_since(begin) >= jiff::SignedDuration::from_hours(24)
            }
            _ => false,
        }
    }
}

/// An event as an exchange's listing shows it.
#[derive(Debug, Clone)]
pub struct EventSummary {
    pub event_id: String,
    pub slug: String,
    pub name: String,
    pub image: String,
    pub image_credit: String,

    /// The session the listing is about: the one under way or next to
    /// begin, or, for an event that is over, the last one it held. `None`
    /// for an event that never gave its dates.
    pub session: Option<Session>,

    /// How many sessions the event has besides `session`.
    pub other_sessions: usize,

    pub location: Where,
}

impl EventSummary {
    /// Fill in `session` and `other_sessions` for a page of results in one
    /// query.
    async fn attach_sessions(events: &mut [EventSummary], now: &str) -> Result<(), Error> {
        if events.is_empty() {
            return Ok(());
        }

        let mut query_builder = sqlx::QueryBuilder::new(
            r#"SELECT "event_id", "begin", "end" FROM event_instances WHERE "event_id" IN ("#,
        );
        let mut separated = query_builder.separated(", ");
        for event in events.iter() {
            separated.push_bind(event.event_id.clone());
        }
        query_builder.push(r#") ORDER BY "begin""#);

        let mut sessions: HashMap<String, Vec<Session>> = HashMap::new();
        for (event_id, begin, end) in query_builder
            .build_query_as::<(String, String, String)>()
            .fetch_all(common::shared::db())
            .await?
        {
            sessions
                .entry(event_id)
                .or_default()
                .push(Session { begin, end });
        }

        for event in events.iter_mut() {
            let Some(held) = sessions.get(&event.event_id) else {
                continue;
            };

            // The first that has not ended, else the last that did.
            let chosen = held
                .iter()
                .find(|session| session.end.as_str() >= now)
                .or_else(|| held.iter().max_by(|a, b| a.end.cmp(&b.end)));

            event.session = chosen.cloned();
            event.other_sessions = held.len().saturating_sub(1);
        }

        Ok(())
    }
}

/// Narrow a query over `events ev` to what `exchange` lists for `params`.
fn push_conditions(
    query_builder: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    exchange: &ExchangeInfo,
    params: &SearchParams,
    scope: &SearchScope,
) {
    if scope.has_text_search() {
        query_builder.push(" JOIN events_fts f ON f.event_id = ev.event_id");
    }

    query_builder.push(" WHERE ent.publish = 1");

    if scope.has_text_search() {
        query_builder.push(" AND events_fts MATCH ");
        query_builder.push_bind(scope.phrases.join(" OR "));
    }

    push_hidden_exclusion(query_builder, &EVENTS, exchange.exchange_id, scope);
    push_membership(query_builder, &EVENTS, exchange, params.global);
    push_named_regions(query_builder, &EVENTS, &scope.regions);

    if !params.past {
        query_builder
            .push(" AND ev.event_id IN (SELECT event_id FROM event_instances WHERE \"end\" >= ");
        query_builder.push_bind(scope.now.clone());
        query_builder.push(")");
    }
}

/// Events `exchange` lists for `params`, a page at a time, and how many
/// there are in all.
async fn find(
    exchange: &ExchangeInfo,
    params: &SearchParams,
    include_hidden: bool,
    page: usize,
) -> Result<(Vec<EventSummary>, usize), Error> {
    if params.filters_projects_only() {
        return Ok((Vec::new(), 0));
    }

    let scope = SearchScope::resolve(params, include_hidden).await?;

    const FROM: &str = r#"
        FROM events ev
        JOIN entities ent ON ent.target = ev.event_id AND ent.kind = 'Event'"#;

    let mut query_builder = sqlx::QueryBuilder::new(
        r#"SELECT
             ev.event_id,
             ent.slug,
             ev.name,
             ev.image,
             ev.image_credit,
             ev.location_type,
             ev.entirely_online,
             ev.location_description,
             (ev.regions IS NOT NULL),
             ev.city,
             ev.state,
             ev.country,
             ev.address"#,
    );
    query_builder.push(FROM);
    push_conditions(&mut query_builder, exchange, params, &scope);

    // Soonest first; once past events are included, the most recently ended
    // follow, and an event that never gave its dates comes last.
    query_builder.push(r#" ORDER BY (SELECT MIN(i."begin") FROM event_instances i WHERE i.event_id = ev.event_id AND i."end" >= "#);
    query_builder.push_bind(scope.now.clone());
    query_builder.push(
        r#") ASC NULLS LAST,
           (SELECT MAX(i."end") FROM event_instances i WHERE i.event_id = ev.event_id) DESC NULLS LAST,
           ev.event_id
         LIMIT "#,
    );
    query_builder.push_bind(PAGE_SIZE as i64);
    query_builder.push(" OFFSET ");
    query_builder.push_bind(((page - 1) * PAGE_SIZE) as i64);

    type Row = (
        String,
        String,
        String,
        String,
        String,
        String,
        bool,
        String,
        bool,
        String,
        String,
        String,
        String,
    );

    let mut events: Vec<EventSummary> = query_builder
        .build_query_as::<Row>()
        .fetch_all(common::shared::db())
        .await?
        .into_iter()
        .map(
            |(
                event_id,
                slug,
                name,
                image,
                image_credit,
                location_type,
                entirely_online,
                location_description,
                coverage,
                city,
                state,
                country,
                address,
            )| EventSummary {
                event_id,
                slug,
                name,
                image,
                image_credit,
                session: None,
                other_sessions: 0,
                location: if entirely_online {
                    Where::Online
                } else {
                    Where::new(
                        &location_type,
                        "",
                        &location_description,
                        coverage,
                        &city,
                        &state,
                        &country,
                        &address,
                    )
                },
            },
        )
        .collect();

    EventSummary::attach_sessions(&mut events, &scope.now).await?;

    let mut count_builder = sqlx::QueryBuilder::new("SELECT COUNT(*)");
    count_builder.push(FROM);
    push_conditions(&mut count_builder, exchange, params, &scope);

    let (total,) = count_builder
        .build_query_as::<(i64,)>()
        .fetch_one(common::shared::db())
        .await?;

    Ok((events, total as usize))
}

#[derive(TemplateOnce)]
#[template(path = "exchange/eventcard.stpl.html")]
struct EventCard<'a> {
    exchange: &'a TemplateExchange,
    event: &'a EventSummary,
}

/// One page of an exchange's events, with its own pager.
#[derive(TemplateOnce)]
#[template(path = "exchange/eventresults.stpl.html")]
pub struct EventResults<'a> {
    exchange: &'a TemplateExchange,
    params: &'a SearchParams,
    events: Vec<EventSummary>,
    page: usize,
    total: usize,
}

impl<'a> EventResults<'a> {
    /// Page `page` of the events `exchange` lists for `params`.
    pub async fn find(
        exchange: &'a TemplateExchange,
        params: &'a SearchParams,
        page: usize,
    ) -> Result<EventResults<'a>, Error> {
        let page = page.max(1);
        let (events, total) =
            find(&exchange.info, params, exchange.user_is_authorized, page).await?;

        Ok(EventResults {
            exchange,
            params,
            events,
            page,
            total,
        })
    }
}

/// The first page of the events a search finds, as the results pane shows
/// them above its projects: nothing at all when there are none.
pub async fn first_events(
    exchange: &TemplateExchange,
    params: &SearchParams,
) -> Result<String, Error> {
    let results = EventResults::find(exchange, params, 1).await?;

    if results.total == 0 {
        Ok(String::new())
    } else {
        Ok(results.render_once()?)
    }
}

/// Another page of an exchange's events, for its pager.
#[handler]
pub async fn exchange_events(
    Path(exchange_id): Path<i64>,
    crate::form::QsQuery(params): crate::form::QsQuery<SearchParams>,
    auth: Authenticated,
) -> Result<poem::Response, Error> {
    let exchange_info = ExchangeInfo::load_by_id(exchange_id).await?;
    let exchange = create_template_exchange(exchange_info, &auth).await?;

    let results = EventResults::find(&exchange, &params, params.page).await?;

    Ok(Html(results.render_once()?).into_response())
}
