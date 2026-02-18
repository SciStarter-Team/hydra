use anyhow::Result;
use common::{
    message::log::{
        ActsOfScienceCreateRequest, ActsOfScienceCreateResponse, ActsOfScienceSource,
        ActsToDateRequest, ActsToDateResponse, AffiliateParticipantsRequest,
        AffiliateParticipantsResponse, AnalyticsQuery, AnalyticsResponse,
        ArticulateCompletionRecord, ArticulateCompletionRequest, ArticulateCompletionResponse,
        ExternalLinkClickthroughResponse, ExternalLinksResponse, GoUrlStatsRequest,
        GoUrlStatsResponse, ProjectParticipationRecord, ProjectParticipationRequest,
        ProjectParticipationResponse, RecentActivityRequest, RecentActivityResponse,
    },
    service::decode_structured_message,
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
    get_top_performers(database, query).await.map_err(Into::into)
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

pub async fn handle_articulate_completion_record(
    req: &async_nats::service::Request,
) -> Result<()> {
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
    sqlx::query!(
        r#"
        INSERT OR IGNORE INTO project_participations_cache (user_id, project_id)
        VALUES (?, ?)
        "#,
        user_id_str,
        record.project_id
    )
    .execute(common::shared::db())
    .await?;

    // Record to participation table for permanent record
    use log_service::database::tables::ParticipationRow;
    let row = ParticipationRow {
        origin: record.origin.unwrap_or(0),                  // 0 = web
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
    let is_april = when_str
        .get(5..7)
        .map(|m| m == "04")
        .unwrap_or(false);
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
        let when = request.when.unwrap_or_else(|| TimeString::now().to_string());
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
        if today.month() > 6 {
            y + 1
        } else {
            y
        }
    });

    let begin = format!("{}-04-01T00:00:00Z", year);
    let end = format!("{}-05-04T00:00:00Z", year);
    let incidental = ActsOfScienceSource::Incidental as i16;

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

    Ok(ActsToDateResponse {
        date: today.to_string(),
        number: total,
        date_reached: None,
    })
}
