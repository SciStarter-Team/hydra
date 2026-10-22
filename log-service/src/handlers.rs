use anyhow::Result;
use common::message::dashboard::{EntitySummary, EntitySummaryKind};
use common::timestring::TimeString;
use common::{
    message::log::{
        ActsOfScienceCreateRequest, ActsOfScienceCreateResponse, ActsOfScienceSource,
        ActsToDateRequest, ActsToDateResponse, AffiliateParticipantsRequest,
        AffiliateParticipantsResponse, AnalyticsQuery, AnalyticsResponse,
        ArticulateCompletionRecord, ArticulateCompletionRequest, ArticulateCompletionResponse,
        EditLogEntry, EditLogRequest, EditLogResponse, EntityListContributions,
        EntityListParticipant, EntityListStatsRequest, EntityListStatsResponse,
        ExternalLinkClickthroughResponse, ExternalLinksResponse, GoUrlStatsRequest,
        GoUrlStatsResponse, LastChangedRequest, LastChangedResponse, PageChange,
        ProjectLikesRequest, ProjectLikesResponse, ProjectMemberCountRequest,
        ProjectMemberCountResponse, ProjectParticipationRecord, ProjectParticipationRequest,
        ProjectParticipationResponse, ProjectReviewsRequest, ProjectReviewsResponse,
        RatingSetRequest, RecentActivityRequest, RecentActivityResponse, ReviewEntry,
        ReviewSetRequest, ReviewersByProjectsRequest, ReviewersByProjectsResponse,
        UserLastViewRequest, UserLastViewResponse, UserParticipationBreakdownEntry,
        UserParticipationBreakdownRequest, UserParticipationBreakdownResponse,
        UserParticipationEntry, UserParticipationRequest, UserParticipationResponse,
        UserRecentVisit, UserRecentVisitsRequest, UserRecentVisitsResponse,
    },
    query::{QUERY_TIMEOUT, QueryRequest, QueryResponse},
    service::{
        TriggerEvent, decode_structured_message, request as nats_request, send, trigger_email,
    },
};
use datafusion::prelude::*;
use ulid::Ulid;

use log_service::{
    analytics::{
        get_clickthrough_analytics_realtime, get_go_url_stats, get_go_url_totals,
        get_last_active_for, get_last_changed, get_participation_analytics_realtime,
        get_participation_by_email, get_participation_by_users, get_participation_summary,
        get_recent_activity, get_top_performers, get_user_first_participation,
        get_users_active_since,
    },
    cache::{update_user_emails_cache, update_user_tags_cache},
    health::HealthResponse,
};
// Import the global database instance from main
use crate::DATABASE;

pub async fn handle_activity_log(req: &async_nats::service::Request) -> Result<()> {
    let (when, action, user_id, target, extra): (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = decode_structured_message(req.message.clone())?;

    use log_service::database::tables::ActivityLogRow;

    let row = ActivityLogRow {
        when,
        action,
        user_id,
        target,
        extra,
    };

    // Clone database reference and drop it immediately after use
    {
        let database = DATABASE
            .read()
            .as_ref()
            .expect("Database not initialized")
            .clone();
        database.activity_log.insert(row).await?
    };

    Ok(())
}

pub async fn handle_page_view(req: &async_nats::service::Request) -> Result<()> {
    let (when, ip_address, device_id, interaction_id, user_id, path, referrer, query, geolocation): (
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        String,
    ) = decode_structured_message(req.message.clone())?;

    use log_service::database::tables::PageViewRow;

    let row = PageViewRow {
        when,
        ip_address,
        device_id,
        interaction_id,
        user_id,
        path,
        referrer,
        query,
        geolocation,
    };

    // Clone database reference and drop it immediately after use
    {
        let database = DATABASE
            .read()
            .as_ref()
            .expect("Database not initialized")
            .clone();
        database.page_views.insert(row).await?
    };

    Ok(())
}

pub async fn handle_clickthrough(req: &async_nats::service::Request) -> Result<()> {
    let (
        when,
        ip_address,
        device_id,
        interaction_id,
        user_id,
        from_path,
        to_url,
        link_type,
        program_id,
        geolocation,
    ): (
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
        Option<String>,
        String,
    ) = decode_structured_message(req.message.clone())?;

    use log_service::database::tables::ClickthroughRow;

    let row = ClickthroughRow {
        when,
        ip_address,
        device_id,
        interaction_id,
        user_id,
        from_path,
        to_url,
        link_type,
        program_id,
        geolocation,
    };

    // Clone database reference and drop it immediately after use
    {
        let database = DATABASE
            .read()
            .as_ref()
            .expect("Database not initialized")
            .clone();
        database.clickthroughs.insert(row).await?
    };

    Ok(())
}

pub async fn handle_share(req: &async_nats::service::Request) -> Result<()> {
    let (when, target_id, platform, user_id, device_id, interaction_id): (
        String,
        String,
        String,
        Option<String>,
        String,
        String,
    ) = decode_structured_message(req.message.clone())?;

    use log_service::database::tables::ShareEventRow;

    let row = ShareEventRow {
        when,
        target_id,
        platform,
        user_id,
        device_id,
        interaction_id,
    };

    // Clone database reference and drop it immediately after use
    {
        let database = DATABASE
            .read()
            .as_ref()
            .expect("Database not initialized")
            .clone();
        database.share_events.insert(row).await?
    };

    Ok(())
}

pub async fn handle_join(req: &async_nats::service::Request) -> Result<()> {
    let (when, entity_id, user_id, active, consented, contacted, notified, activity, reason): (
        String,
        String,
        String,
        bool,
        Option<String>,
        Option<String>,
        bool,
        String,
        String,
    ) = decode_structured_message(req.message.clone())?;

    use log_service::database::tables::JoinRow;

    let row = JoinRow {
        when,
        entity_id,
        user_id,
        active,
        consented,
        contacted,
        explicit: true, // Live joins are always explicit
        notified,
        activity,
        reason,
    };

    // Clone database reference and drop it immediately after use
    {
        let database = DATABASE
            .read()
            .as_ref()
            .expect("Database not initialized")
            .clone();
        database.join_table.insert(row).await?
    };

    Ok(())
}

pub async fn handle_clickthrough_analytics(
    req: &async_nats::service::Request,
) -> Result<AnalyticsResponse, anyhow::Error> {
    let query: AnalyticsQuery = decode_structured_message(req.message.clone())?;

    // Clone database reference and use it in scope
    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    get_clickthrough_analytics_realtime(database, query)
        .await
        .map_err(Into::into)
}

/// Ad-hoc read-only SQL over the DataFusion analytics context, the
/// Parquet-side counterpart to the `log.query.select` endpoint that
/// [`common::query`] serves over the SQLite cache.
pub async fn handle_analytics_query(
    req: &async_nats::service::Request,
) -> Result<QueryResponse, anyhow::Error> {
    let request: QueryRequest = decode_structured_message(req.message.clone())?;

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();

    tokio::time::timeout(
        QUERY_TIMEOUT,
        log_service::query::execute(database.context(), &request),
    )
    .await
    .map_err(|_| anyhow::anyhow!("query timed out after {} seconds", QUERY_TIMEOUT.as_secs()))?
    .map_err(Into::into)
}

pub async fn handle_participation_analytics(
    req: &async_nats::service::Request,
) -> Result<AnalyticsResponse, anyhow::Error> {
    let query: AnalyticsQuery = decode_structured_message(req.message.clone())?;

    // Clone database reference and use it in scope
    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    get_participation_analytics_realtime(database, query)
        .await
        .map_err(Into::into)
}

pub async fn handle_participation_summary(
    req: &async_nats::service::Request,
) -> Result<common::message::log::ParticipationSummaryResponse, anyhow::Error> {
    let request: common::message::log::ParticipationSummaryRequest =
        decode_structured_message(req.message.clone())?;

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    get_participation_summary(database, request)
        .await
        .map_err(Into::into)
}

/// All-time clicks across a set of go-links (`log.gourl_totals`).
pub async fn handle_gourl_totals(
    req: &async_nats::service::Request,
) -> Result<common::message::log::GoUrlTotalsResponse, anyhow::Error> {
    let request: common::message::log::GoUrlTotalsRequest =
        decode_structured_message(req.message.clone())?;

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    get_go_url_totals(database, request)
        .await
        .map_err(Into::into)
}

/// Per-person, per-project participation counts for the curated-page staff
/// report (`log.participation_by_users`).
pub async fn handle_participation_by_users(
    req: &async_nats::service::Request,
) -> Result<common::message::log::ParticipationByUsersResponse, anyhow::Error> {
    let request: common::message::log::ParticipationByUsersRequest =
        decode_structured_message(req.message.clone())?;

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    get_participation_by_users(database, request)
        .await
        .map_err(Into::into)
}

pub async fn handle_user_first_participation(
    req: &async_nats::service::Request,
) -> Result<common::message::log::UserFirstParticipationResponse, anyhow::Error> {
    let request: common::message::log::UserFirstParticipationRequest =
        decode_structured_message(req.message.clone())?;

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    get_user_first_participation(database, request)
        .await
        .map_err(Into::into)
}

pub async fn handle_participation_by_email(
    req: &async_nats::service::Request,
) -> Result<common::message::log::ParticipationByEmailResponse, anyhow::Error> {
    let request: common::message::log::ParticipationByEmailRequest =
        decode_structured_message(req.message.clone())?;

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    get_participation_by_email(database, request)
        .await
        .map_err(Into::into)
}

pub async fn handle_top_performers_analytics(
    req: &async_nats::service::Request,
) -> Result<AnalyticsResponse, anyhow::Error> {
    let query: common::message::log::TopPerformersQuery =
        decode_structured_message(req.message.clone())?;

    // Clone database reference and use it in scope
    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    get_top_performers(database, query)
        .await
        .map_err(Into::into)
}

pub async fn handle_user_tags_update(req: &async_nats::service::Request) -> Result<()> {
    let (user_id, tags): (String, Vec<String>) = decode_structured_message(req.message.clone())?;

    loop {
        if update_user_tags_cache(user_id.clone(), tags.clone())
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    Ok(())
}

pub async fn handle_user_emails_update(req: &async_nats::service::Request) -> Result<()> {
    let (user_id, emails): (String, Vec<String>) = decode_structured_message(req.message.clone())?;

    // Update cache using SQLite-based cache system with retry logic
    loop {
        if update_user_emails_cache(user_id.clone(), emails.clone())
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    Ok(())
}

pub async fn handle_health_check(
    req: &async_nats::service::Request,
) -> Result<HealthResponse, anyhow::Error> {
    // Clone database reference and use it in scope
    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    log_service::health::health_check(database, req).await
}

pub async fn handle_recent_activity(
    req: &async_nats::service::Request,
) -> Result<RecentActivityResponse, anyhow::Error> {
    let request: RecentActivityRequest = decode_structured_message(req.message.clone())?;

    // Clone database reference and use it in scope
    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    get_recent_activity(database, request)
        .await
        .map_err(Into::into)
}

pub async fn handle_go_url_stats(
    req: &async_nats::service::Request,
) -> Result<GoUrlStatsResponse, anyhow::Error> {
    let request: GoUrlStatsRequest = decode_structured_message(req.message.clone())?;

    // Clone database reference and use it in scope
    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();
    get_go_url_stats(database, request)
        .await
        .map_err(Into::into)
}

pub async fn handle_email_event(req: &async_nats::service::Request) -> Result<()> {
    let (when, event_type, sending_id, email_address, user_id, reason): (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    ) = decode_structured_message(req.message.clone())?;

    use log_service::database::tables::EmailEventRow;

    let row = EmailEventRow {
        when,
        event_type,
        sending_id,
        email_address,
        user_id,
        reason,
    };

    // Clone database reference and drop it immediately after use
    {
        let database = DATABASE
            .read()
            .as_ref()
            .expect("Database not initialized")
            .clone();
        database.email_events.insert(row).await?
    };

    Ok(())
}

pub async fn handle_search_event(req: &async_nats::service::Request) -> Result<()> {
    use common::message::log::SearchEvent;
    use log_service::database::tables::SearchEventRow;

    let event: SearchEvent = decode_structured_message(req.message.clone())?;

    let row = SearchEventRow {
        when: event.when,
        device_id: event.device_id,
        interaction_id: event.interaction_id,
        user_id: event.user_id,
        query_text: event.query_text,
        lat: event.lat,
        lng: event.lng,
        location_name: event.location_name,
        location_type: event.location_type,
        topics: event.topics,
        audiences: event.audiences,
        activities: event.activities,
        misc_filters: event.misc_filters,
        show_completed: event.show_completed,
        kinds: event.kinds,
        show_past: Some(event.show_past),
        event_start: event.event_start,
        event_end: event.event_end,
        page: event.page,
        result_count: event.result_count,
        search_type: event.search_type,
    };

    {
        let database = DATABASE
            .read()
            .as_ref()
            .expect("Database not initialized")
            .clone();
        database.search_events.insert(row).await?
    };

    Ok(())
}

pub async fn handle_affiliate_participation(
    req: &async_nats::service::Request,
) -> Result<AffiliateParticipantsResponse, anyhow::Error> {
    let request: AffiliateParticipantsRequest = decode_structured_message(req.message.clone())?;

    // Clone database reference and use it in scope
    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();

    let ctx = database.context();

    // Convert ULID and TimeString to strings for the query
    let affiliate_id_str = request.affiliate_id.to_string();
    let start_str = request.start.to_string();
    let end_str = request.end.to_string();

    // Query participation table for users who participated in this affiliate within the date range
    let df = ctx
        .table("participation")
        .await?
        .filter(col("entity_id").eq(lit(&affiliate_id_str)))?
        .filter(col("when").gt_eq(lit(&start_str)))?
        .filter(col("when").lt_eq(lit(&end_str)))?
        .filter(col("user_id").is_not_null())?
        .select(vec![col("user_id")])?
        .distinct()?;

    let batches = df.collect().await?;

    // Extract user IDs from the result batches
    let mut user_ids = Vec::new();
    use datafusion::arrow::array::{Array, StringArray};

    for batch in batches {
        if batch.num_rows() == 0 {
            continue;
        }

        let user_id_array = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("user_id column should be StringArray");

        for i in 0..user_id_array.len() {
            if !user_id_array.is_null(i) {
                if let Ok(user_id) = user_id_array.value(i).parse::<Ulid>() {
                    user_ids.push(user_id);
                }
            }
        }
    }

    Ok(AffiliateParticipantsResponse { user_ids })
}

/// Answer the owner of a list: who opened it, and what did they contribute
/// before and after it existed.
///
/// Both halves are assembled here because both live here. The views are
/// `activity_log` rows written by the list page; the contributions are
/// `participation` rows. Splitting them anywhere else would mean shipping
/// every raw row to the caller.
pub async fn handle_entity_list_stats(
    req: &async_nats::service::Request,
) -> Result<EntityListStatsResponse, anyhow::Error> {
    use datafusion::arrow::array::{Array, Int32Array, StringArray};
    use std::collections::HashMap;

    let request: EntityListStatsRequest = decode_structured_message(req.message.clone())?;

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();

    let ctx = database.context();

    let list_id_str = request.list_id.to_string();

    // Every view of the list, identified or not. The list page logs one per
    // page load, so these are visits rather than distinct people.
    let view_batches = ctx
        .table("activity_log")
        .await?
        .filter(col("action").eq(lit("entity_list_view")))?
        .filter(col("target").eq(lit(&list_id_str)))?
        .select(vec![col("user_id"), col("when")])?
        .collect()
        .await?;

    let mut identified_views = 0i64;
    let mut anonymous_views = 0i64;
    let mut first_views: HashMap<Ulid, String> = HashMap::new();

    for batch in view_batches {
        let users = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("user_id column should be StringArray");
        let whens = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("when column should be StringArray");

        for i in 0..users.len() {
            if users.is_null(i) {
                anonymous_views += 1;
                continue;
            }

            identified_views += 1;

            let Ok(user_id) = users.value(i).parse::<Ulid>() else {
                continue;
            };

            let when = whens.value(i).to_string();

            first_views
                .entry(user_id)
                .and_modify(|earliest| {
                    if when < *earliest {
                        *earliest = when.clone();
                    }
                })
                .or_insert(when);
        }
    }

    let participants: Vec<EntityListParticipant> = first_views
        .iter()
        .map(|(user_id, first_view)| EntityListParticipant {
            user_id: *user_id,
            first_view: first_view.clone(),
        })
        .collect();

    if participants.is_empty() || request.entity_ids.is_empty() {
        return Ok(EntityListStatsResponse {
            identified_views,
            anonymous_views,
            participants,
            contributions: Vec::new(),
        });
    }

    // Contributions to the entities on the list, by the people who opened
    // it. Narrowed to those entities in the query; narrowing to those users
    // as well happens below, since the participant set is usually the
    // smaller filter and is already in hand.
    let entity_ids: Vec<String> = request
        .entity_ids
        .iter()
        .map(|entity_id| entity_id.to_string())
        .collect();

    let participation_batches = ctx
        .table("participation")
        .await?
        .filter(col("user_id").is_not_null())?
        .filter(in_list(
            col("entity_id"),
            entity_ids.iter().map(lit).collect(),
            false,
        ))?
        .select(vec![
            col("user_id"),
            col("entity_id"),
            col("when"),
            col("magnitude"),
        ])?
        .collect()
        .await?;

    // magnitude is the multiplier a single record carries, so a record
    // standing for four contributions counts as four.
    let mut totals: HashMap<(Ulid, Ulid), EntityListContributions> = HashMap::new();

    for batch in participation_batches {
        let users = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("user_id column should be StringArray");
        let entities = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("entity_id column should be StringArray");
        let whens = batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("when column should be StringArray");
        let magnitudes = batch
            .column(3)
            .as_any()
            .downcast_ref::<Int32Array>()
            .expect("magnitude column should be Int32Array");

        for i in 0..users.len() {
            if users.is_null(i) {
                continue;
            }

            let (Ok(user_id), Ok(entity_id)) = (
                users.value(i).parse::<Ulid>(),
                entities.value(i).parse::<Ulid>(),
            ) else {
                continue;
            };

            let Some(first_view) = first_views.get(&user_id) else {
                continue;
            };

            let when = whens.value(i);
            let magnitude = magnitudes.value(i) as i64;

            let entry =
                totals
                    .entry((user_id, entity_id))
                    .or_insert_with(|| EntityListContributions {
                        user_id,
                        entity_id,
                        since_list_creation: 0,
                        before_list_creation: 0,
                        since_viewing: 0,
                    });

            // With no creation date there is nothing to attest that a
            // contribution came after the list existed, so none of it is
            // claimed as such.
            if !request.created_at.is_empty() && when >= request.created_at.as_str() {
                entry.since_list_creation += magnitude;
            } else {
                entry.before_list_creation += magnitude;
            }

            if when >= first_view.as_str() {
                entry.since_viewing += magnitude;
            }
        }
    }

    Ok(EntityListStatsResponse {
        identified_views,
        anonymous_views,
        participants,
        contributions: totals.into_values().collect(),
    })
}

pub async fn handle_articulate_completion(
    req: &async_nats::service::Request,
) -> Result<ArticulateCompletionResponse, anyhow::Error> {
    let request: ArticulateCompletionRequest = decode_structured_message(req.message.clone())?;

    // Convert ULID to string for the query
    let user_id_str = request.user_id.to_string();

    // Query cache table for completion
    let result = sqlx::query_scalar!(
        r#"
        SELECT user_id
        FROM articulate_completions_cache
        WHERE user_id = ? AND activity = ?
        LIMIT 1
        "#,
        user_id_str,
        request.activity
    )
    .fetch_optional(common::shared::db())
    .await?;

    let completed = result.is_some();

    Ok(ArticulateCompletionResponse { completed })
}

pub async fn handle_project_participation(
    req: &async_nats::service::Request,
) -> Result<ProjectParticipationResponse, anyhow::Error> {
    let request: ProjectParticipationRequest = decode_structured_message(req.message.clone())?;

    // Convert ULID to string for the query
    let user_id_str = request.user_id.to_string();

    // Query cache table for participation
    let result = sqlx::query_scalar!(
        r#"
        SELECT user_id
        FROM project_participations_cache
        WHERE user_id = ? AND project_id = ?
        LIMIT 1
        "#,
        user_id_str,
        request.project_id
    )
    .fetch_optional(common::shared::db())
    .await?;

    let participated = result.is_some();

    Ok(ProjectParticipationResponse { participated })
}

pub async fn handle_articulate_completion_record(req: &async_nats::service::Request) -> Result<()> {
    let record: ArticulateCompletionRecord = decode_structured_message(req.message.clone())?;

    let user_id_str = record.user_id.to_string();
    let now = common::timestring::TimeString::now().to_string();

    // Insert into cache (ignore if already exists)
    sqlx::query!(
        r#"
        INSERT OR IGNORE INTO articulate_completions_cache (user_id, activity)
        VALUES (?, ?)
        "#,
        user_id_str,
        record.activity
    )
    .execute(common::shared::db())
    .await?;

    // Record to activity_log table for permanent record
    use log_service::database::tables::ActivityLogRow;
    let row = ActivityLogRow {
        when: now.clone(),
        action: "articulate_complete".to_string(),
        user_id: Some(user_id_str.clone()),
        target: None,
        extra: Some(record.activity.clone()),
    };

    {
        let database = DATABASE
            .read()
            .as_ref()
            .expect("Database not initialized")
            .clone();
        database.activity_log.insert(row).await?;
    }

    // Send SSE notification to user
    common::channel::send_to_user(
        record.user_id,
        "completion",
        common::channel::Event::new("articulate_complete", &record.activity),
    )
    .await?;

    Ok(())
}

/// Tell an account about its first contribution, once, ever.
///
/// Orrery's rule, which this has to match: the mail goes out on a first
/// contribution to a project, but only to an account that has never been sent
/// it, and never for the two projects that are nobody's first act of science
/// -- the Foundations module's own page, which stands for finishing a
/// training rather than for taking part in anything, and iSeeChange.
///
/// The `first-contribution` tag is the record that it was sent, and the guard
/// against sending it again. It is also mirrored to Constant Contact as an
/// engagement segment, so it has to be written whenever the mail goes out;
/// writing it refreshes this service's copy of the account's tags too, so a
/// second contribution arriving moments later sees it.
async fn announce_first_contribution(
    user_id: &str,
    record: &ProjectParticipationRecord,
) -> Result<()> {
    const TAG: &str = "first-contribution";

    if matches!(
        crate::project_ids::slug_of(&record.project_id).await?,
        Some(crate::project_ids::FOUNDATIONS_MODULE | crate::project_ids::ISEECHANGE)
    ) {
        return Ok(());
    }

    if log_service::cache::get_user_tags(user_id)
        .await?
        .iter()
        .any(|tag| tag == TAG)
    {
        return Ok(());
    }

    // The mail names the project and links its page, so the id alone is not
    // enough. The entity tables live in the frontend's database; ask it.
    let summary: Option<EntitySummary> =
        nats_request("frontend.entity.summary_by_id", record.project_id.clone())
            .await
            .unwrap_or(None);

    let mut fields = vec![("project_id".to_string(), record.project_id.clone())];

    if let Some(summary) = summary {
        // The same two canonical paths the frontend's `entity_link` builds;
        // that helper lives in the frontend crate and is not reachable here.
        let prefix = match summary.kind {
            EntitySummaryKind::Project => "projects",
            EntitySummaryKind::Event => "events",
        };
        fields.push((
            "project_page_url".to_string(),
            format!("https://{}/{prefix}/{}", env!("DOMAIN"), summary.slug),
        ));
        fields.push(("project_name".to_string(), summary.name));
    }

    trigger_email(
        TriggerEvent::FirstContribution,
        record.user_id,
        Some(&fields),
    )
    .await?;

    send("user.tags.add", (user_id.to_string(), TAG.to_string())).await?;

    Ok(())
}

pub async fn handle_project_participation_record(req: &async_nats::service::Request) -> Result<()> {
    let record: ProjectParticipationRecord = decode_structured_message(req.message.clone())?;

    let user_id_str = record.user_id.to_string();

    // Insert into cache (ignore if already exists)
    let cache_result = sqlx::query!(
        r#"
        INSERT OR IGNORE INTO project_participations_cache (user_id, project_id)
        VALUES (?, ?)
        "#,
        user_id_str,
        record.project_id
    )
    .execute(common::shared::db())
    .await?;

    // A new row means this is the account's first contribution to this
    // project, which is the first of the two things the mail below asks.
    if cache_result.rows_affected() > 0
        && let Err(error) = announce_first_contribution(&user_id_str, &record).await
    {
        eprintln!("First contribution by {user_id_str} went unannounced: {error}");
    }

    // Record to participation table for permanent record
    use log_service::database::tables::ParticipationRow;
    let row = ParticipationRow {
        origin: record.origin.unwrap_or(0), // 0 = web
        // Every participation is recorded through here, so here is where a
        // partner's spelling of the time becomes one canonical instant.
        when: common::contribution_time::recorded_contribution_time(
            record.when.as_deref(),
            &record.project_id,
        )
        .into(),
        duration: record.duration.unwrap_or(1.0),
        where_point: record.where_point,
        magnitude: record.magnitude.unwrap_or(1),
        authenticated: record.authenticated.unwrap_or(true),
        extra: record.extra.unwrap_or_default(),
        fidelity: record.fidelity.unwrap_or(1.0),
        entity_id: record.project_id.clone(),
        user_id: Some(user_id_str.clone()),
        task_name: record.task_name,
    };

    let when_str = row.when.clone();

    {
        let database = DATABASE
            .read()
            .as_ref()
            .expect("Database not initialized")
            .clone();
        database.participation.insert(row).await?;
    }

    // Send SSE notification to user
    common::channel::send_to_user(
        record.user_id,
        "completion",
        common::channel::Event::new("project_participation", &record.project_id),
    )
    .await?;

    // Participating in one of the Foundations module's projects is a step of
    // the module. Not fatal: the contribution is already recorded and will not
    // be offered again, so failing here would lose it to buy nothing.
    if let Err(error) =
        crate::foundations::advance_for_participation(&user_id_str, &record.project_id).await
    {
        eprintln!(
            "Participation by {user_id_str} in {} did not advance the Foundations module: {error}",
            record.project_id
        );
    }

    // If it's April, automatically create an Acts of Science entry
    let is_april = when_str.get(5..7).map(|m| m == "04").unwrap_or(false);
    if is_april {
        let act_id = Ulid::new().to_string();
        let source = ActsOfScienceSource::Affiliate as i16;
        sqlx::query!(
            r#"INSERT OR IGNORE INTO acts_of_science
               (act_id, user_id, created_at, contributions, source, entity_slug, accepted)
               VALUES (?, ?, ?, 1, ?, ?, 1)"#,
            act_id,
            user_id_str,
            when_str,
            source,
            record.project_id,
        )
        .execute(common::shared::db())
        .await
        .ok();
    }

    Ok(())
}

pub async fn handle_external_links(
    req: &async_nats::service::Request,
) -> Result<ExternalLinksResponse, anyhow::Error> {
    use common::message::log::ExternalLinksRequest;

    let request: ExternalLinksRequest = decode_structured_message(req.message.clone())?;

    let db = common::shared::db();
    let mut links = std::collections::HashMap::new();

    // Use a transaction for atomicity
    let mut tx = db.begin().await?;

    for url in request.urls {
        // Try to get existing link for this slug+url combination
        let existing = sqlx::query!(
            "SELECT link_id FROM external_links WHERE slug = ? AND url = ?",
            request.slug,
            url
        )
        .fetch_optional(&mut *tx)
        .await?;

        let link_id = if let Some(row) = existing {
            row.link_id.parse::<ulid::Ulid>()?
        } else {
            // Create new link
            let new_id = ulid::Ulid::new();
            let new_id_str = new_id.to_string();
            let now = TimeString::now().to_string();

            sqlx::query!(
                "INSERT INTO external_links (link_id, slug, url, created_at) VALUES (?, ?, ?, ?)",
                new_id_str,
                request.slug,
                url,
                now
            )
            .execute(&mut *tx)
            .await?;

            new_id
        };

        links.insert(url, link_id);
    }

    tx.commit().await?;

    Ok(ExternalLinksResponse { links })
}

pub async fn handle_external_link_clickthrough(
    req: &async_nats::service::Request,
) -> Result<ExternalLinkClickthroughResponse, anyhow::Error> {
    use common::message::log::ExternalLinkClickthroughRequest;
    use common::timestring::TimeString;
    use log_service::database::tables::ClickthroughRow;

    let request: ExternalLinkClickthroughRequest = decode_structured_message(req.message.clone())?;

    let db = common::shared::db();

    // Use a transaction for atomicity
    let mut tx = db.begin().await?;

    // Get URL and slug for this link_id
    let link_id_str = request.link_id.to_string();
    let link = sqlx::query!(
        "SELECT slug, url FROM external_links WHERE link_id = ?",
        link_id_str
    )
    .fetch_optional(&mut *tx)
    .await?;

    let (slug, url) = match link {
        Some(row) => (row.slug, Some(row.url)),
        None => (String::new(), None),
    };

    // If URL found, record the clickthrough
    if let Some(ref url_string) = url {
        let when = request
            .when
            .unwrap_or_else(|| TimeString::now().to_string());
        let user_id = request.user_id.map(|id| id.to_string());

        // Resolve geolocation from IP if not provided
        let geolocation = match request.geolocation {
            Some(geo) => geo,
            None => {
                match common::service::request::<common::message::location::LocationInfo>(
                    "location.ip",
                    &request.ip_address,
                )
                .await
                {
                    Ok(loc) => loc
                        .as_point()
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| r#"{"type":"Point","coordinates":[0,0]}"#.to_string()),
                    Err(_) => r#"{"type":"Point","coordinates":[0,0]}"#.to_string(),
                }
            }
        };

        let row = ClickthroughRow {
            when,
            ip_address: request.ip_address,
            device_id: request.device_id,
            interaction_id: request.interaction_id,
            user_id,
            from_path: format!("/{}", slug),
            to_url: url_string.to_string(),
            link_type: request.link_type,
            program_id: request.program_id,
            geolocation,
        };

        let database = DATABASE
            .read()
            .as_ref()
            .expect("Database not initialized")
            .clone();
        database.clickthroughs.insert(row).await?;
    }

    tx.commit().await?;

    Ok(ExternalLinkClickthroughResponse { url })
}

fn is_user_id(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_digit()) || Ulid::from_string(s).is_ok()
}

pub async fn handle_contributors(
    req: &async_nats::service::Request,
) -> Result<common::message::log::ContributorsResponse, anyhow::Error> {
    use common::message::log::{ContributorEntry, ContributorsRequest, ContributorsResponse};

    let request: ContributorsRequest = decode_structured_message(req.message.clone())?;

    let mode = match request.mode.as_deref() {
        Some(m) if !m.is_empty() => m.to_string(),
        _ => {
            return Ok(ContributorsResponse {
                contributors: vec![],
                pages: 0,
            });
        }
    };

    let letter = request.letter.clone().unwrap_or_default().to_uppercase();

    // For name mode, require a letter
    if mode == "name" && letter.is_empty() {
        return Ok(ContributorsResponse {
            contributors: vec![],
            pages: 0,
        });
    }

    let page = request.page.unwrap_or(1).max(1);
    const PAGE_SIZE: i32 = 50;
    let offset = (page - 1) * PAGE_SIZE;

    let today = jiff::Zoned::now().date();
    let month: i32 = today.month().into();
    let year: i32 = today.year().into();
    let csm_year = if month > 6 { year + 1 } else { year };
    let begin = format!("{}-04-01T00:00:00Z", csm_year);
    let end = format!("{}-05-04T00:00:00Z", csm_year);
    let incidental = ActsOfScienceSource::Incidental as i16;

    let db = common::shared::db();

    // Sampled acts are dropped at an 85% rate, so roughly 15% of sampled
    // rows survive into the grouping step. The score is a Knuth-constant
    // multiplicative hash of the epoch seconds, which avalanches adjacent
    // seconds into different buckets while staying deterministic per row
    // so the count and fetch queries agree and pagination is stable.
    const SAMPLING_FILTER: &str = " AND (sampled = 0 OR (CAST(strftime('%s', created_at) AS INTEGER) * 2654435761 % 100) < 15)";

    // Build the query dynamically based on mode
    // Use GROUP BY to deduplicate on (name, act, project, date)
    let mut qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new(
        "SELECT name, act, project, MAX(organization) as organization, MAX(entity_slug) as entity_slug, DATE(created_at) as date, MAX(featured) as featured FROM acts_of_science WHERE accepted = 1 AND display = 1 AND source != ",
    );
    qb.push_bind(incidental);
    qb.push(" AND created_at >= ");
    qb.push_bind(begin.clone());
    qb.push(" AND created_at < ");
    qb.push_bind(end.clone());
    qb.push(SAMPLING_FILTER);

    match mode.as_str() {
        "name" => {
            qb.push(" AND UPPER(SUBSTR(name, 1, 1)) = ");
            qb.push_bind(letter.clone());
            qb.push(" GROUP BY name, act, project, date ORDER BY featured DESC, name, date DESC");
        }
        "project" => {
            qb.push(" AND project != '' AND project IS NOT NULL GROUP BY name, act, project, date ORDER BY featured DESC, project, date DESC");
        }
        "org" => {
            qb.push(" AND organization != '' AND organization IS NOT NULL GROUP BY name, act, project, date ORDER BY featured DESC, MAX(organization), date DESC");
        }
        "date" | _ => {
            qb.push(" GROUP BY name, act, project, date ORDER BY featured DESC, date DESC");
        }
    }

    // Count query — count distinct (name, act, project, date) tuples that
    // survive the sampling filter
    let mut count_qb: sqlx::QueryBuilder<sqlx::Sqlite> = sqlx::QueryBuilder::new(
        "SELECT COUNT(*) FROM (SELECT DISTINCT name, act, project, DATE(created_at) as date FROM acts_of_science WHERE accepted = 1 AND display = 1 AND source != ",
    );
    count_qb.push_bind(incidental);
    count_qb.push(" AND created_at >= ");
    count_qb.push_bind(begin);
    count_qb.push(" AND created_at < ");
    count_qb.push_bind(end);
    count_qb.push(SAMPLING_FILTER);

    match mode.as_str() {
        "name" => {
            count_qb.push(" AND UPPER(SUBSTR(name, 1, 1)) = ");
            count_qb.push_bind(letter);
        }
        "project" => {
            count_qb.push(" AND project != '' AND project IS NOT NULL");
        }
        "org" => {
            count_qb.push(" AND organization != '' AND organization IS NOT NULL");
        }
        _ => {}
    }
    count_qb.push(")");

    let total: i64 = count_qb.build_query_scalar().fetch_one(db).await?;

    let pages = ((total as f64) / (PAGE_SIZE as f64)).ceil() as i32;

    // Add pagination
    qb.push(" LIMIT ");
    qb.push_bind(PAGE_SIZE);
    qb.push(" OFFSET ");
    qb.push_bind(offset);

    let rows = qb
        .build_query_as::<(
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
        )>()
        .fetch_all(db)
        .await?;

    let contributors: Vec<ContributorEntry> = rows
        .into_iter()
        .map(|r| ContributorEntry {
            name: r.0.map(|n| {
                if is_user_id(&n) {
                    "anon".to_string()
                } else {
                    n
                }
            }),
            act: r.1,
            project: r.2,
            organization: r.3,
            entity_slug: r.4,
            date: r.5,
            featured: r.6.unwrap_or(0) != 0,
        })
        .collect();

    Ok(ContributorsResponse {
        contributors,
        pages,
    })
}

pub async fn handle_acts_of_science_create(
    req: &async_nats::service::Request,
) -> Result<ActsOfScienceCreateResponse, anyhow::Error> {
    let request: ActsOfScienceCreateRequest = decode_structured_message(req.message.clone())?;

    let act_id = Ulid::new().to_string();
    let user_id = request.user_id.map(|u| u.to_string());
    let source = request.source as i16;
    let newsletter = if request.newsletter { 1i32 } else { 0i32 };
    let contributions = request.contributions;
    let num_others = request.num_others;

    sqlx::query!(
        r#"INSERT INTO acts_of_science
           (act_id, user_id, email, country, newsletter, name, contributions, act,
            organization, num_others, others, project, notes, source, entity_slug, accepted)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0)"#,
        act_id,
        user_id,
        request.email,
        request.country,
        newsletter,
        request.name,
        contributions,
        request.act,
        request.organization,
        num_others,
        request.others,
        request.project,
        request.notes,
        source,
        request.entity_slug,
    )
    .execute(common::shared::db())
    .await?;

    Ok(ActsOfScienceCreateResponse { act_id })
}

pub async fn handle_acts_to_date(
    req: &async_nats::service::Request,
) -> Result<ActsToDateResponse, anyhow::Error> {
    let request: ActsToDateRequest = decode_structured_message(req.message.clone())?;

    let today = jiff::Zoned::now().date();
    let year = request.year.unwrap_or_else(|| {
        let y: i32 = today.year().into();
        if today.month() > 6 { y + 1 } else { y }
    });

    let begin = format!("{}-04-01T00:00:00Z", year);
    let end = format!("{}-05-07T00:00:00Z", year);
    let incidental = ActsOfScienceSource::Incidental as i16;

    // IMPORTANT filtering on accepted state is important, with human
    // acceptance. Do not change to automatic acceptance or ignore the
    // state.
    let total: i64 = sqlx::query_scalar!(
        r#"SELECT COALESCE(SUM(contributions), 0) as "total!: i64"
           FROM acts_of_science
           WHERE accepted = 1
             AND source != ?
             AND created_at >= ?
             AND created_at < ?"#,
        incidental,
        begin,
        end,
    )
    .fetch_one(common::shared::db())
    .await?;

    const CSM_GOAL: i64 = 2_500_000;

    let date_reached: Option<String> = sqlx::query_scalar!(
        r#"WITH daily AS (
               SELECT date(created_at) AS day,
                      SUM(contributions) AS daily_total
               FROM acts_of_science
               WHERE accepted = 1
                 AND source != ?
                 AND created_at >= ?
                 AND created_at < ?
               GROUP BY date(created_at)
           ),
           cumulative AS (
               SELECT day,
                      SUM(daily_total) OVER (ORDER BY day) AS running_total
               FROM daily
           )
           SELECT day AS "day!: String"
           FROM cumulative
           WHERE running_total >= ?
           ORDER BY day
           LIMIT 1"#,
        incidental,
        begin,
        end,
        CSM_GOAL,
    )
    .fetch_optional(common::shared::db())
    .await?;

    let cutoff = jiff::civil::date(year as i16, 4, 30);
    let response_date = if today > cutoff { cutoff } else { today };

    Ok(ActsToDateResponse {
        date: response_date.to_string(),
        number: total,
        date_reached,
    })
}

pub async fn handle_project_member_count(
    req: &async_nats::service::Request,
) -> Result<ProjectMemberCountResponse> {
    let request: ProjectMemberCountRequest = decode_structured_message(req.message.clone())?;

    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64" FROM project_participations_cache WHERE project_id = ?"#,
        request.project_id
    )
    .fetch_one(common::shared::db())
    .await?;

    Ok(ProjectMemberCountResponse { count })
}

/// Compute a project's like count and, for an optional user, whether they liked it.
async fn project_likes(project_id: &str, user_id: Option<&str>) -> Result<ProjectLikesResponse> {
    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64"
           FROM project_ratings_cache
           WHERE project_id = ? AND amount > 0"#,
        project_id
    )
    .fetch_one(common::shared::db())
    .await?;

    let liked = if let Some(user_id) = user_id {
        sqlx::query_scalar!(
            r#"SELECT EXISTS(
                 SELECT 1 FROM project_ratings_cache
                 WHERE project_id = ? AND user_id = ? AND amount > 0
               ) as "liked!: bool""#,
            project_id,
            user_id
        )
        .fetch_one(common::shared::db())
        .await?
    } else {
        false
    };

    Ok(ProjectLikesResponse { count, liked })
}

pub async fn handle_project_likes(
    req: &async_nats::service::Request,
) -> Result<ProjectLikesResponse> {
    let request: ProjectLikesRequest = decode_structured_message(req.message.clone())?;
    project_likes(&request.project_id, request.user_id.as_deref()).await
}

pub async fn handle_rating_set(req: &async_nats::service::Request) -> Result<ProjectLikesResponse> {
    let request: RatingSetRequest = decode_structured_message(req.message.clone())?;

    let amount: i64 = if request.liked { 1 } else { 0 };
    let when = common::timestring::TimeString::now().to_string();

    sqlx::query!(
        r#"INSERT INTO project_ratings_cache (project_id, user_id, amount, "when")
           VALUES (?, ?, ?, ?)
           ON CONFLICT (project_id, user_id)
           DO UPDATE SET amount = excluded.amount, "when" = excluded."when""#,
        request.project_id,
        request.user_id,
        amount,
        when
    )
    .execute(common::shared::db())
    .await?;

    project_likes(&request.project_id, Some(&request.user_id)).await
}

pub async fn handle_project_reviews(
    req: &async_nats::service::Request,
) -> Result<ProjectReviewsResponse> {
    let request: ProjectReviewsRequest = decode_structured_message(req.message.clone())?;

    let reviews = sqlx::query_as!(
        ReviewEntry,
        r#"SELECT user_id, text, "when"
           FROM project_reviews_cache
           WHERE project_id = ? AND private = 0
           ORDER BY "when" DESC
           LIMIT ?"#,
        request.project_id,
        request.limit
    )
    .fetch_all(common::shared::db())
    .await?;

    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64"
           FROM project_reviews_cache
           WHERE project_id = ? AND private = 0"#,
        request.project_id
    )
    .fetch_one(common::shared::db())
    .await?;

    Ok(ProjectReviewsResponse { reviews, count })
}

/// Everyone who has reviewed any of the named projects
/// (`log.reviewers_by_projects`).
///
/// Private reviews are included: the staff report this backs asks who cared
/// about a project, not what a visitor can read.
pub async fn handle_reviewers_by_projects(
    req: &async_nats::service::Request,
) -> Result<ReviewersByProjectsResponse> {
    use sqlx::QueryBuilder;

    let request: ReviewersByProjectsRequest = decode_structured_message(req.message.clone())?;

    if request.project_ids.is_empty() {
        return Ok(ReviewersByProjectsResponse::default());
    }

    let mut user_ids = std::collections::BTreeSet::new();

    // SQLite caps a statement at 999 bound parameters by default.
    for chunk in request.project_ids.chunks(900) {
        let mut builder = QueryBuilder::new(
            "SELECT DISTINCT user_id FROM project_reviews_cache WHERE project_id IN (",
        );
        let mut separated = builder.separated(", ");
        for id in chunk {
            separated.push_bind(id);
        }
        builder.push(")");

        let rows: Vec<String> = builder
            .build_query_scalar()
            .fetch_all(common::shared::db())
            .await?;

        user_ids.extend(rows);
    }

    Ok(ReviewersByProjectsResponse {
        user_ids: user_ids.into_iter().collect(),
    })
}

pub async fn handle_review_set(req: &async_nats::service::Request) -> Result<ReviewEntry> {
    let request: ReviewSetRequest = decode_structured_message(req.message.clone())?;

    let private: i64 = if request.private { 1 } else { 0 };
    let when = common::timestring::TimeString::now().to_string();

    // Preserve orrery's `checked` flag on edit (it defaults to 0 for new reviews).
    sqlx::query!(
        r#"INSERT INTO project_reviews_cache (project_id, user_id, text, private, "when")
           VALUES (?, ?, ?, ?, ?)
           ON CONFLICT (project_id, user_id)
           DO UPDATE SET text = excluded.text, private = excluded.private, "when" = excluded."when""#,
        request.project_id,
        request.user_id,
        request.text,
        private,
        when
    )
    .execute(common::shared::db())
    .await?;

    Ok(ReviewEntry {
        user_id: request.user_id,
        text: request.text,
        when,
    })
}

/// When each of the requested targets last changed: the latest
/// [`PageChange`] logged against it.
pub async fn handle_last_changed(
    req: &async_nats::service::Request,
) -> Result<LastChangedResponse> {
    let request: LastChangedRequest = decode_structured_message(req.message.clone())?;

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();

    get_last_changed(database, request)
        .await
        .map_err(Into::into)
}

pub async fn handle_user_participation(
    req: &async_nats::service::Request,
) -> Result<UserParticipationResponse> {
    use datafusion::arrow::array::{Array, StringArray};

    let request: UserParticipationRequest = decode_structured_message(req.message.clone())?;
    let user_id_str = request.user_id.to_string();

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();

    let ctx = database.context();

    let total: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64"
           FROM project_participations_cache
           WHERE user_id = ?"#,
        user_id_str,
    )
    .fetch_one(common::shared::db())
    .await?;

    let recent_limit = request.recent_limit.max(0) as usize;
    let mut recent = Vec::new();

    if recent_limit > 0 {
        let df = ctx
            .table("participation_realtime")
            .await?
            .filter(col("user_id").eq(lit(&user_id_str)))?
            .select(vec![col("entity_id"), col("when")])?
            .sort(vec![col("when").sort(false, true)])?
            .limit(0, Some(recent_limit))?;

        let batches = df.collect().await?;

        for batch in &batches {
            let entity_col = batch
                .column(0)
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("entity_id column should be StringArray");
            let when_col = batch
                .column(1)
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("when column should be StringArray");

            for i in 0..batch.num_rows() {
                if entity_col.is_null(i) || when_col.is_null(i) {
                    continue;
                }
                recent.push(UserParticipationEntry {
                    entity_id: entity_col.value(i).to_string(),
                    when: when_col.value(i).to_string(),
                });
            }
        }
    }

    let recent_days = request.recent_days.max(0);
    let mut recent_count = 0;

    if recent_days > 0 {
        let cutoff = jiff::Zoned::now()
            .checked_sub(jiff::Span::new().days(recent_days))
            .map(|z| z.timestamp().to_string())
            .unwrap_or_default();

        recent_count = ctx
            .table("participation_realtime")
            .await?
            .filter(
                col("user_id")
                    .eq(lit(&user_id_str))
                    .and(col("when").gt_eq(lit(cutoff))),
            )?
            .count()
            .await? as i64;
    }

    Ok(UserParticipationResponse {
        total,
        recent,
        recent_count,
    })
}

/// Count the user's contributions per entity within the trailing
/// `days` window, most-active entity first.
pub async fn handle_user_participation_breakdown(
    req: &async_nats::service::Request,
) -> Result<UserParticipationBreakdownResponse> {
    use datafusion::arrow::array::{Array, StringArray};
    use std::collections::HashMap;

    let request: UserParticipationBreakdownRequest =
        decode_structured_message(req.message.clone())?;
    let user_id_str = request.user_id.to_string();

    let cutoff = jiff::Zoned::now()
        .checked_sub(jiff::Span::new().days(request.days.max(0)))
        .map(|z| z.timestamp().to_string())
        .unwrap_or_default();

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();

    let ctx = database.context();

    let df = ctx
        .table("participation_realtime")
        .await?
        .filter(
            col("user_id")
                .eq(lit(&user_id_str))
                .and(col("when").gt_eq(lit(&cutoff))),
        )?
        .select(vec![col("entity_id")])?;

    let batches = df.collect().await?;

    let mut counts: HashMap<String, i64> = HashMap::new();
    for batch in &batches {
        let entity_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("entity_id column should be StringArray");
        for i in 0..batch.num_rows() {
            if entity_col.is_null(i) {
                continue;
            }
            *counts.entry(entity_col.value(i).to_string()).or_insert(0) += 1;
        }
    }

    let mut entries: Vec<UserParticipationBreakdownEntry> = counts
        .into_iter()
        .map(|(entity_id, count)| UserParticipationBreakdownEntry { entity_id, count })
        .collect();
    entries.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.entity_id.cmp(&b.entity_id))
    });

    Ok(UserParticipationBreakdownResponse { entries })
}

pub async fn handle_user_recent_visits(
    req: &async_nats::service::Request,
) -> Result<UserRecentVisitsResponse> {
    use datafusion::arrow::array::{Array, StringArray};

    let request: UserRecentVisitsRequest = decode_structured_message(req.message.clone())?;
    let user_id_str = request.user_id.to_string();
    let limit = request.limit.max(0) as usize;

    if limit == 0 {
        return Ok(UserRecentVisitsResponse { visits: vec![] });
    }

    let cutoff = jiff::Zoned::now()
        .checked_sub(jiff::Span::new().days(request.days.max(0)))
        .map(|z| z.timestamp().to_string())
        .unwrap_or_else(|_| String::new());

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();

    let ctx = database.context();

    // Pull a generous window of candidate page-views, then dedupe by
    // path keeping only the most recent visit. Caller is responsible
    // for filtering paths down to entity slugs.
    let candidate_window = (limit * 8).max(64);

    let df = ctx
        .table("page_views_realtime")
        .await?
        .filter(col("user_id").eq(lit(&user_id_str)))?
        .filter(col("when").gt_eq(lit(&cutoff)))?
        .select(vec![col("path"), col("when")])?
        .sort(vec![col("when").sort(false, true)])?
        .limit(0, Some(candidate_window))?;

    let batches = df.collect().await?;

    let mut visits = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for batch in &batches {
        let path_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("path column should be StringArray");
        let when_col = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("when column should be StringArray");

        for i in 0..batch.num_rows() {
            if path_col.is_null(i) || when_col.is_null(i) {
                continue;
            }
            let path = path_col.value(i).to_string();
            if !seen.insert(path.clone()) {
                continue;
            }
            visits.push(UserRecentVisit {
                path,
                when: when_col.value(i).to_string(),
            });
            if visits.len() >= limit {
                return Ok(UserRecentVisitsResponse { visits });
            }
        }
    }

    Ok(UserRecentVisitsResponse { visits })
}

/// The timestamp of the user's most recent page view, unbounded by any recency
/// window (unlike `handle_user_recent_visits`). Returns `None` when the user
/// has no recorded page views.
pub async fn handle_user_last_view(
    req: &async_nats::service::Request,
) -> Result<UserLastViewResponse> {
    use datafusion::arrow::array::{Array, StringArray};

    let request: UserLastViewRequest = decode_structured_message(req.message.clone())?;
    let user_id_str = request.user_id.to_string();

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();

    let ctx = database.context();

    let df = ctx
        .table("page_views_realtime")
        .await?
        .filter(col("user_id").eq(lit(&user_id_str)))?
        .select(vec![col("when")])?
        .sort(vec![col("when").sort(false, true)])?
        .limit(0, Some(1))?;

    let batches = df.collect().await?;

    let when = batches.iter().find_map(|batch| {
        let when_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("when column should be StringArray");
        (batch.num_rows() > 0 && !when_col.is_null(0)).then(|| when_col.value(0).to_string())
    });

    Ok(UserLastViewResponse { when })
}

pub async fn handle_edit_log(req: &async_nats::service::Request) -> Result<EditLogResponse> {
    use datafusion::arrow::array::{Array, StringArray};

    let request: EditLogRequest = decode_structured_message(req.message.clone())?;

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();

    let ctx = database.context();

    let df = ctx
        .table("activity_log_realtime")
        .await?
        .filter(
            col("action")
                .eq(lit(PageChange::ProjectEdit.action()))
                .or(col("action").eq(lit(PageChange::EventEdit.action()))),
        )?
        .filter(col("target").eq(lit(&request.target_id)))?
        .select(vec![col("when"), col("user_id"), col("extra")])?
        .sort(vec![col("when").sort(false, true)])?
        .limit(0, Some(request.limit.max(1) as usize))?;

    let batches = df.collect().await?;

    let mut entries = Vec::new();
    for batch in &batches {
        let when_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let user_col = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let extra_col = batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        for i in 0..batch.num_rows() {
            entries.push(EditLogEntry {
                when: if when_col.is_null(i) {
                    String::new()
                } else {
                    when_col.value(i).to_string()
                },
                user_id: if user_col.is_null(i) {
                    String::new()
                } else {
                    user_col.value(i).to_string()
                },
                field: if extra_col.is_null(i) {
                    String::new()
                } else {
                    extra_col.value(i).to_string()
                },
            });
        }
    }

    Ok(EditLogResponse { entries })
}

/// The accounts seen since a given moment, for the Constant Contact sweep's
/// work list.
pub async fn handle_users_active_since(
    req: &async_nats::service::Request,
) -> Result<common::message::log::UsersActiveSinceResponse, anyhow::Error> {
    let request: common::message::log::UsersActiveSinceRequest =
        decode_structured_message(req.message.clone())?;

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();

    get_users_active_since(database, request)
        .await
        .map_err(Into::into)
}

/// When each of a batch of accounts was last seen.
pub async fn handle_last_active_for(
    req: &async_nats::service::Request,
) -> Result<common::message::log::LastActiveForResponse, anyhow::Error> {
    let request: common::message::log::LastActiveForRequest =
        decode_structured_message(req.message.clone())?;

    let database = DATABASE
        .read()
        .as_ref()
        .expect("Database not initialized")
        .clone();

    get_last_active_for(database, request)
        .await
        .map_err(Into::into)
}
