//! Each project's stats page (`log.project_stats`), answered from a snapshot
//! of every project's numbers that is rebuilt once a day.
//!
//! What a project is compared with -- the median, mean, maximum and
//! percentile of every project's figure -- is the same for every project and
//! costs a pass over every log to work out, so it is worked out once a day
//! for all of them ([`snapshot`]), along with each project's own totals and
//! last 90 days. A request then reads a handful of indexed rows. The
//! snapshot counts whole UTC days, ending yesterday.

pub mod snapshot;

use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::Result;
use common::{
    message::participants::{ContributionsRequest, ParticipantContributions},
    message::project_stats::{
        AgeGroup, Audience, Comparison, DayCounts, Feedback, Gender, Measure, MeasureStats,
        ProjectStats, ProjectStatsRequest, ProjectStatsResponse, ShareStats, Window,
    },
    service::decode_structured_message,
    shared::db,
};
use jiff::{Timestamp, ToSpan, civil::Date, tz::TimeZone};
use log_service::Database;
use tokio_util::sync::CancellationToken;
use ulid::Ulid;

use snapshot::{
    DAYS_KEPT, FEEDBACK_JOINS, FEEDBACK_JOINS_PERCENT, FEEDBACK_UNKNOWN, FEEDBACK_UNKNOWN_PERCENT,
    Series,
};

/// The UTC hour the snapshot is rebuilt at, once the day before has ended
/// and the hourly syncs have had time to bring in the end of it.
const HOUR: i8 = 2;

/// How long a failed build waits before it is tried again.
const RETRY: Duration = Duration::from_secs(60 * 60);

/// The measures the page shows, and the windows it shows each over.
const SHOWN: [(Measure, &[Window]); 5] = [
    (Measure::Views, &[Window::AllTime, Window::Last30]),
    (Measure::Visits, &[Window::AllTime, Window::Last30]),
    (Measure::Saves, &[Window::AllTime, Window::Last30]),
    (Measure::Joins, &[Window::AllTime, Window::Last30]),
    (
        Measure::Contributions,
        &[Window::AllTime, Window::Last7, Window::Last30],
    ),
];

/// The windows shares are shown over.
const SHARE_WINDOWS: [Window; 2] = [Window::AllTime, Window::Last30];

/// How many other projects the audience panel names.
const ALSO_VIEWED: i64 = 10;

/// Keep the snapshot current: build it now if it does not count yesterday,
/// then again after each day ends.
pub async fn run_daily(database: Arc<Database>, cancellation_token: CancellationToken) {
    loop {
        let now = Timestamp::now();

        let wait = match stale(now).await {
            Ok(false) => until_next_build(now),
            Ok(true) => match snapshot::build(&database.context(), now).await {
                Ok(()) => {
                    println!("Project stats snapshot built");
                    until_next_build(Timestamp::now())
                }
                Err(e) => {
                    eprintln!("Building the project stats snapshot failed: {e:#}");
                    RETRY
                }
            },
            Err(e) => {
                eprintln!("Reading the project stats snapshot failed: {e:#}");
                RETRY
            }
        };

        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = cancellation_token.cancelled() => {
                println!("Project stats snapshot cancelled, exiting gracefully");
                return;
            }
        }
    }
}

/// Whether the snapshot does not yet count the last whole day as of `now`.
async fn stale(now: Timestamp) -> Result<bool> {
    let through = sqlx::query_scalar!(r#"SELECT "through" FROM "project_stats_snapshot""#)
        .fetch_optional(db())
        .await?;

    let due = snapshot::through_for(now)?.to_string();

    Ok(through.is_none_or(|through| through < due))
}

/// How long from `now` until the next build is due.
fn until_next_build(now: Timestamp) -> Duration {
    let today = now.to_zoned(TimeZone::UTC).date();
    let due = |date: Date| {
        date.at(HOUR, 0, 0, 0)
            .to_zoned(TimeZone::UTC)
            .map(|zoned| zoned.timestamp())
    };

    let next = match due(today) {
        Ok(at) if at > now => Ok(at),
        _ => today.tomorrow().and_then(due),
    };

    next.ok()
        .and_then(|next| Duration::try_from(next.duration_since(now)).ok())
        .unwrap_or(RETRY)
}

pub async fn handle_project_stats(
    req: &async_nats::service::Request,
) -> Result<ProjectStatsResponse> {
    let request: ProjectStatsRequest = decode_structured_message(req.message.clone())?;

    let Some(snapshot) = sqlx::query!(r#"SELECT "built", "through" FROM "project_stats_snapshot""#)
        .fetch_optional(db())
        .await?
    else {
        return Ok(ProjectStatsResponse::Pending);
    };

    let through: Date = snapshot.through.parse()?;
    let project_id = request.project_id.to_string();

    let totals: HashMap<(String, String), i64> = sqlx::query!(
        r#"SELECT "series", "span", "total" FROM "project_stats_totals"
           WHERE "project_id" = ?"#,
        project_id,
    )
    .fetch_all(db())
    .await?
    .into_iter()
    .map(|row| ((row.series, row.span), row.total))
    .collect();

    let distributions: HashMap<(String, String), Vec<f64>> = sqlx::query!(
        r#"SELECT "series", "span", "sorted_values" FROM "project_stats_distributions""#
    )
    .fetch_all(db())
    .await?
    .into_iter()
    .map(|row| {
        Ok((
            (row.series, row.span),
            serde_json::from_str(&row.sorted_values)?,
        ))
    })
    .collect::<Result<_>>()?;

    let compare = |series: &str, window: Window, value: f64| {
        let domain = distributions
            .get(&(series.to_string(), window.key().to_string()))
            .map(Vec::as_slice)
            .unwrap_or_default();
        Comparison::against(value, domain)
    };
    let total = |series: &str, window: Window| {
        totals
            .get(&(series.to_string(), window.key().to_string()))
            .copied()
            .unwrap_or(0)
    };

    let measures = SHOWN
        .iter()
        .flat_map(|(measure, windows)| windows.iter().map(move |window| (*measure, *window)))
        .map(|(measure, window)| MeasureStats {
            measure,
            window,
            comparison: compare(measure.key(), window, total(measure.key(), window) as f64),
        })
        .collect();

    // Every platform anything has been shared to, so that a project shared
    // to none of them still shows each with nothing.
    let mut platforms: Vec<String> = distributions
        .keys()
        .chain(totals.keys())
        .filter_map(|(series, _)| series.strip_prefix(Series::SHARE_PREFIX))
        .map(str::to_string)
        .collect();
    platforms.sort_unstable();
    platforms.dedup();

    let shares = SHARE_WINDOWS
        .into_iter()
        .map(|window| {
            let platforms: Vec<(String, Comparison)> = platforms
                .iter()
                .map(|platform| {
                    let series = Series::Share(platform.clone()).key();
                    let comparison = compare(&series, window, total(&series, window) as f64);
                    (platform.clone(), comparison)
                })
                .collect();

            ShareStats {
                window,
                total: platforms
                    .iter()
                    .map(|(_, comparison)| comparison.value as i64)
                    .sum(),
                platforms,
            }
        })
        .collect();

    let feedback = {
        let (joined, unknown) = sqlx::query!(
            r#"SELECT "joined", "unknown" FROM "project_stats_feedback"
               WHERE "project_id" = ?"#,
            project_id,
        )
        .fetch_optional(db())
        .await?
        .map(|row| (row.joined as f64, row.unknown as f64))
        .unwrap_or_default();

        let all = joined + unknown;
        let percent = |part: f64| if all > 0.0 { 100.0 * part / all } else { 0.0 };

        Feedback {
            joins: compare(FEEDBACK_JOINS, Window::AllTime, joined),
            joins_percent: compare(FEEDBACK_JOINS_PERCENT, Window::AllTime, percent(joined)),
            unknown: compare(FEEDBACK_UNKNOWN, Window::AllTime, unknown),
            unknown_percent: compare(FEEDBACK_UNKNOWN_PERCENT, Window::AllTime, percent(unknown)),
        }
    };

    Ok(ProjectStatsResponse::Ready(Box::new(ProjectStats {
        updated: snapshot.built,
        through: snapshot.through,
        measures,
        days: days(&project_id, through).await?,
        shares,
        audience: audience(&project_id, through).await?,
        feedback,
    })))
}

/// `log.project_participants`: what each of the asked-about participants
/// has contributed to the project, as of the snapshot.
pub async fn handle_project_participants(
    req: &async_nats::service::Request,
) -> Result<Vec<ParticipantContributions>> {
    let request: ContributionsRequest = decode_structured_message(req.message.clone())?;
    let project_id = request.project_id.to_string();
    let mut found = Vec::with_capacity(request.user_ids.len());

    for user_id in request.user_ids {
        let user = user_id.to_string();
        let row = sqlx::query!(
            r#"SELECT "contributions", "last_contribution" FROM "project_stats_participants"
               WHERE "project_id" = ? AND "user_id" = ?"#,
            project_id,
            user,
        )
        .fetch_optional(db())
        .await?;

        if let Some(row) = row {
            found.push(ParticipantContributions {
                user_id,
                contributions: row.contributions,
                last: row.last_contribution,
            });
        }
    }

    Ok(found)
}

/// The project's counts for each of the last [`DAYS_KEPT`] days through
/// `through`, oldest first, days with nothing to count included.
async fn days(project_id: &str, through: Date) -> Result<Vec<DayCounts>> {
    let first = snapshot::first_day(through, DAYS_KEPT)?;
    let first_day = first.to_string();

    let mut counted: HashMap<String, DayCounts> = sqlx::query_as!(
        DayCounts,
        r#"SELECT "day", "views", "visits", "saves", "contributions"
           FROM "project_stats_days"
           WHERE "project_id" = ? AND "day" >= ?"#,
        project_id,
        first_day,
    )
    .fetch_all(db())
    .await?
    .into_iter()
    .map(|counts| (counts.day.clone(), counts))
    .collect();

    let mut days = Vec::with_capacity(DAYS_KEPT as usize);
    let mut day = first;
    while day <= through {
        let key = day.to_string();
        days.push(counted.remove(&key).unwrap_or(DayCounts {
            day: key,
            ..DayCounts::default()
        }));
        day = day.checked_add(1.day())?;
    }

    Ok(days)
}

/// Who views the project, and what else they view.
async fn audience(project_id: &str, through: Date) -> Result<Audience> {
    let on = through.to_string();

    let viewers = sqlx::query!(
        r#"SELECT v."views", p."gender" AS "gender?", p."birthday"
           FROM "project_stats_viewers" v
           LEFT JOIN "project_stats_people" p ON p."user_id" = v."user_id"
           WHERE v."project_id" = ?"#,
        project_id,
    )
    .fetch_all(db())
    .await?;

    let mut by_gender: HashMap<Gender, i64> = HashMap::new();
    let mut by_age: HashMap<AgeGroup, i64> = HashMap::new();
    let mut views = 0;

    for viewer in viewers {
        let gender = viewer
            .gender
            .as_deref()
            .map(Gender::from_key)
            .unwrap_or(Gender::Unknown);
        let age = AgeGroup::of(viewer.birthday.as_deref(), &on);

        views += viewer.views;
        *by_gender.entry(gender).or_default() += viewer.views;
        *by_age.entry(age).or_default() += viewer.views;
    }

    let also_viewed = sqlx::query!(
        r#"SELECT o."project_id", SUM(o."views") AS "views!: i64"
           FROM "project_stats_viewers" v
           JOIN "project_stats_viewers" o
             ON o."user_id" = v."user_id" AND o."project_id" <> v."project_id"
           JOIN "project_stats_projects" p
             ON p."project_id" = o."project_id" AND p."published"
           WHERE v."project_id" = ?
           GROUP BY o."project_id"
           ORDER BY 2 DESC, o."project_id"
           LIMIT ?"#,
        project_id,
        ALSO_VIEWED,
    )
    .fetch_all(db())
    .await?
    .into_iter()
    .filter_map(|row| Some((row.project_id.parse::<Ulid>().ok()?, row.views)))
    .collect();

    Ok(Audience {
        views,
        by_gender: Gender::ALL
            .into_iter()
            .map(|gender| (gender, by_gender.get(&gender).copied().unwrap_or(0)))
            .collect(),
        by_age: AgeGroup::ALL
            .into_iter()
            .map(|age| (age, by_age.get(&age).copied().unwrap_or(0)))
            .collect(),
        also_viewed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    #[test]
    fn the_next_build_is_at_two_utc() {
        let before: Timestamp = "2026-11-17T01:00:00Z".parse().unwrap();
        let after: Timestamp = "2026-11-17T03:00:00Z".parse().unwrap();

        check!(until_next_build(before) == Duration::from_secs(60 * 60));
        check!(until_next_build(after) == Duration::from_secs(23 * 60 * 60));
    }
}
