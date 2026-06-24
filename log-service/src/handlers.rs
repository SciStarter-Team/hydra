use anyhow::Result;
use common::{
    message::log::{
        ActsOfScienceCreateRequest, ActsOfScienceCreateResponse, ActsOfScienceSource,
        ActsToDateRequest, ActsToDateResponse, AffiliateParticipantsRequest,
        AffiliateParticipantsResponse, AnalyticsQuery, AnalyticsResponse,
        ArticulateCompletionRecord, ArticulateCompletionRequest, ArticulateCompletionResponse,
        EditLogEntry, EditLogRequest, EditLogResponse, ExternalLinkClickthroughResponse,
        ExternalLinksResponse, GoUrlStatsRequest, GoUrlStatsResponse, LastEditedRequest,
        LastEditedResponse, ProjectLikesRequest, ProjectLikesResponse, ProjectMemberCountRequest,
        ProjectMemberCountResponse, ProjectParticipationRecord, ProjectParticipationRequest,
        ProjectParticipationResponse, ProjectReviewsRequest, ProjectReviewsResponse,
        RatingSetRequest, RecentActivityRequest, RecentActivityResponse, ReviewEntry,
        ReviewSetRequest, UserParticipationEntry, UserParticipationRequest,
        UserParticipationResponse, UserRecentVisit, UserRecentVisitsRequest,
        UserRecentVisitsResponse,
    },
    service::{TriggerEvent, decode_structured_message, trigger_email},
};
use datafusion::prelude::*;
use ulid::Ulid;

use log_service::{
    analytics::{
        get_clickthrough_analytics_realtime, get_go_url_stats,
        get_participation_analytics_realtime, get_recent_activity, get_top_performers,
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

pub async fn handle_project_participation_record(req: &async_nats::service::Request) -> Result<()> {
    let record: ProjectParticipationRecord = decode_structured_message(req.message.clone())?;

    let user_id_str = record.user_id.to_string();
    let now = common::timestring::TimeString::now().to_string();

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

    // If this was a new row, it's the user's first contribution to this project
    if cache_result.rows_affected() > 0 {
        trigger_email(
            TriggerEvent::FirstContribution,
            record.user_id,
            Some(&[("project_id".to_string(), record.project_id.clone())]),
        )
        .await
        .ok();
    }

    // Record to participation table for permanent record
    use log_service::database::tables::ParticipationRow;
    let row = ParticipationRow {
        origin: record.origin.unwrap_or(0), // 0 = web
        when: record.when.unwrap_or_else(|| now.clone()),
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
    use common::timestring::TimeString;

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

pub async fn handle_last_edited(req: &async_nats::service::Request) -> Result<LastEditedResponse> {
    use datafusion::arrow::array::{Array, StringArray};

    let request: LastEditedRequest = decode_structured_message(req.message.clone())?;

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
                .eq(lit("project-edit"))
                .or(col("action").eq(lit("event-edit"))),
        )?
        .filter(col("target").eq(lit(&request.target_id)))?
        .select(vec![col("when")])?
        .sort(vec![col("when").sort(false, true)])?
        .limit(0, Some(1))?;

    let batches = df.collect().await?;

    let when = batches
        .first()
        .filter(|b| b.num_rows() > 0)
        .and_then(|b| {
            b.column(0)
                .as_any()
                .downcast_ref::<StringArray>()
                .and_then(|a| {
                    if a.is_null(0) {
                        None
                    } else {
                        Some(a.value(0).to_string())
                    }
                })
        })
        .unwrap_or_default();

    Ok(LastEditedResponse { when })
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

    Ok(UserParticipationResponse { total, recent })
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
                .eq(lit("project-edit"))
                .or(col("action").eq(lit("event-edit"))),
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
