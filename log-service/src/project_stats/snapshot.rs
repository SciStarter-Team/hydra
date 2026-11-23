//! Building the daily snapshot of every project's stats.
//!
//! Every source is read once, in full, and tallied per project and day; the
//! totals, the last 90 days, the distributions every project is compared
//! with, and who viewed what are then written over the previous snapshot in
//! one transaction, so a reader sees one snapshot or the other.
//!
//! Where the counts come from:
//!
//! - Views: orrery's daily view counts (`orrery_project_views`) and, from
//!   cutover on, the page views logged here, matched to a project by the slug
//!   in their path.
//! - Visits: orrery's visits, which the join sync logged as `join`
//!   clickthroughs, and, from cutover on, clicks on a project page's Visit
//!   button and Visit clicks on an exchange's pages.
//! - Saves and joins: the user service, which holds them.
//! - Shares: orrery's share clicks (`orrery_shares`) and, from cutover on,
//!   the share log.
//! - Contributions: the participation log, one row per repetition, leaving
//!   out rows that report a join.
//!
//! What hydra logged before cutover is testing, so the views, visits and
//! shares it logged itself only count from the moment it became the primary
//! site, which the user service records.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::{Context, Result};
use common::{
    message::project_stats::{
        DayCounts, Gender, JoinPair, Measure, ORRERY_VISIT_LINK_TYPE, PageRequest, Person,
        ProjectSlugs, Save, VISIT_LINK_TYPE, Window, page_slug,
    },
    service::request,
    shared::db,
    timestring::TimeString,
    transaction::Immediate,
};
use datafusion::arrow::{
    array::{Array, Int64Array, StringArray},
    compute::cast,
    datatypes::DataType,
    record_batch::RecordBatch,
};
use datafusion::prelude::SessionContext;
use futures::TryStreamExt;
use jiff::{Timestamp, ToSpan, civil::Date, tz::TimeZone};
use log_service::database::tables::ParticipationRow;
use ulid::Ulid;

/// Rows asked for per page of a feed from another service.
const PAGE: u32 = 5_000;

/// How many days the snapshot keeps day by day.
pub const DAYS_KEPT: i64 = 90;

/// Rows written per INSERT statement.
const BATCH: usize = 500;

/// The spans a total is kept over.
pub const WINDOWS: [Window; 3] = [Window::AllTime, Window::Last30, Window::Last7];

/// A series of counts: one of the measures, or shares to one platform.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Series {
    Measure(Measure),
    Share(String),
}

impl Series {
    pub const SHARE_PREFIX: &'static str = "share:";

    pub fn key(&self) -> String {
        match self {
            Series::Measure(measure) => measure.key().to_string(),
            Series::Share(platform) => format!("{}{platform}", Series::SHARE_PREFIX),
        }
    }
}

/// The feedback figures that are compared, by the series they are kept as.
pub const FEEDBACK_JOINS: &str = "feedback:joins";
pub const FEEDBACK_UNKNOWN: &str = "feedback:unknown";
pub const FEEDBACK_JOINS_PERCENT: &str = "feedback:joins_percent";
pub const FEEDBACK_UNKNOWN_PERCENT: &str = "feedback:unknown_percent";

/// The last day the snapshot built at `now` counts: the last whole UTC day.
pub fn through_for(now: Timestamp) -> Result<Date> {
    Ok(now.to_zoned(TimeZone::UTC).date().yesterday()?)
}

/// The first day of the window of `days` days ending `through`.
pub fn first_day(through: Date, days: i64) -> Result<Date> {
    Ok(through.checked_sub((days - 1).days())?)
}

/// A total over each window.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Tally {
    all_time: i64,
    last_30: i64,
    last_7: i64,
}

impl Tally {
    fn get(self, window: Window) -> i64 {
        match window {
            Window::AllTime => self.all_time,
            Window::Last30 => self.last_30,
            Window::Last7 => self.last_7,
        }
    }
}

/// Everything the snapshot holds, before it is written.
#[derive(Debug, Default)]
pub struct Tallies {
    through: String,
    first_7: String,
    first_30: String,
    first_kept: String,
    published: HashMap<Ulid, bool>,
    by_slug: HashMap<String, Ulid>,
    totals: HashMap<(Series, Ulid), Tally>,
    days: BTreeMap<(Ulid, String), DayCounts>,
    viewers: HashMap<(Ulid, Ulid), i64>,
    visitors: HashSet<(Ulid, Ulid)>,
    joiners: HashSet<(Ulid, Ulid)>,
    people: Vec<Person>,
    /// The accounts deleted, whom no row names.
    forgotten: HashSet<Ulid>,
    /// Each signed-in participant's contributions to each project, and when
    /// they last contributed.
    participants: Vec<(Ulid, Ulid, i64, String)>,
}

impl Tallies {
    pub fn new(through: Date, projects: Vec<ProjectSlugs>) -> Result<Tallies> {
        let mut tallies = Tallies {
            through: through.to_string(),
            first_7: first_day(through, 7)?.to_string(),
            first_30: first_day(through, 30)?.to_string(),
            first_kept: first_day(through, DAYS_KEPT)?.to_string(),
            ..Tallies::default()
        };

        for project in projects {
            tallies
                .published
                .insert(project.project_id, project.published);
            for slug in project.slugs {
                tallies.by_slug.insert(slug, project.project_id);
            }
        }

        Ok(tallies)
    }

    /// The project a site path is the page of.
    fn project_at(&self, path: &str) -> Option<Ulid> {
        self.by_slug.get(&page_slug(path)?).copied()
    }

    /// The project an id names, if it is one.
    fn project(&self, id: &str) -> Option<Ulid> {
        id.parse().ok().filter(|id| self.published.contains_key(id))
    }

    /// Count `n` of `series` for `project` on `day` (or the day a longer
    /// timestamp starts with). Days after the snapshot's last are not
    /// counted, nor are dates that are not dates.
    fn count(&mut self, series: Series, project: Ulid, day: &str, n: i64) {
        let Some(day) = day.get(..10) else {
            return;
        };

        if n == 0 || day > self.through.as_str() || day.as_bytes().get(4) != Some(&b'-') {
            return;
        }

        let tally = self.totals.entry((series.clone(), project)).or_default();
        tally.all_time += n;
        if day >= self.first_30.as_str() {
            tally.last_30 += n;
        }
        if day >= self.first_7.as_str() {
            tally.last_7 += n;
        }

        if day < self.first_kept.as_str() {
            return;
        }

        let counts = self
            .days
            .entry((project, day.to_string()))
            .or_insert_with(|| DayCounts {
                day: day.to_string(),
                ..DayCounts::default()
            });

        match series {
            Series::Measure(Measure::Views) => counts.views += n,
            Series::Measure(Measure::Visits) => counts.visits += n,
            Series::Measure(Measure::Saves) => counts.saves += n,
            Series::Measure(Measure::Contributions) => counts.contributions += n,
            Series::Measure(Measure::Joins) | Series::Share(_) => {}
        }
    }

    /// Each project's feedback: how many of the signed-in accounts that went
    /// to its site, or joined it, have joined, and how many have not said.
    fn feedback(&self) -> BTreeMap<Ulid, (i64, i64)> {
        let mut feedback: BTreeMap<Ulid, (i64, i64)> = BTreeMap::new();

        for (project, _) in &self.joiners {
            feedback.entry(*project).or_default().0 += 1;
        }

        for visitor in &self.visitors {
            if !self.joiners.contains(visitor) {
                feedback.entry(visitor.0).or_default().1 += 1;
            }
        }

        feedback
    }

    /// What every project with any of each series has of it over each span,
    /// ascending, and likewise each feedback figure.
    fn distributions(
        &self,
        feedback: &BTreeMap<Ulid, (i64, i64)>,
    ) -> BTreeMap<(String, &'static str), Vec<f64>> {
        let mut distributions: BTreeMap<(String, &'static str), Vec<f64>> = BTreeMap::new();

        for ((series, _), tally) in &self.totals {
            for window in WINDOWS {
                let total = tally.get(window);
                if total > 0 {
                    distributions
                        .entry((series.key(), window.key()))
                        .or_default()
                        .push(total as f64);
                }
            }
        }

        for &(joined, unknown) in feedback.values() {
            let all = (joined + unknown) as f64;
            let span = Window::AllTime.key();

            for (series, value) in [
                (FEEDBACK_JOINS, joined as f64),
                (FEEDBACK_UNKNOWN, unknown as f64),
                (FEEDBACK_JOINS_PERCENT, 100.0 * joined as f64 / all),
                (FEEDBACK_UNKNOWN_PERCENT, 100.0 * unknown as f64 / all),
            ] {
                distributions
                    .entry((series.to_string(), span))
                    .or_default()
                    .push(value);
            }
        }

        for values in distributions.values_mut() {
            values.sort_by(f64::total_cmp);
        }

        distributions
    }
}

/// Build the snapshot as of `now` and write it.
pub async fn build(ctx: &SessionContext, now: Timestamp) -> Result<()> {
    let through = through_for(now)?;
    let projects = paged::<Ulid, ProjectSlugs>("frontend.entity.project_slugs", |project| {
        project.project_id
    })
    .await
    .context("reading the projects' slugs")?;

    let mut tallies = Tallies::new(through, projects)?;

    let primary: Option<TimeString> = request("user.stats.primary_since", ())
        .await
        .context("asking when hydra became primary")?;
    let native = match primary {
        Some(since) => {
            let since = Timestamp::try_from(since).context("reading the primary-since time")?;
            Some(format!(
                r#"to_timestamp("when") >= to_timestamp('{since}')"#
            ))
        }
        None => None,
    };

    count_views(ctx, &mut tallies, native.as_deref()).await?;
    count_visits(ctx, &mut tallies, native.as_deref()).await?;
    count_shares(ctx, &mut tallies, native.as_deref()).await?;
    count_contributions(ctx, &mut tallies).await?;
    count_saves(&mut tallies).await?;
    count_joins(&mut tallies).await?;

    tallies.people = paged::<Ulid, Person>("user.stats.people", |person| person.user_id)
        .await
        .context("reading who viewers are")?;
    tallies.forgotten = crate::forgotten::forgotten()
        .await
        .context("reading the accounts deleted")?;

    write(&tallies, now).await
}

/// Every row of one of another service's paged feeds, `key` naming the
/// cursor each row leaves.
async fn paged<K, T>(subject: &'static str, key: impl Fn(&T) -> K) -> Result<Vec<T>>
where
    K: serde::Serialize + Clone,
    T: serde::de::DeserializeOwned,
{
    let mut rows = Vec::new();
    let mut page = PageRequest::<K>::first(PAGE);

    loop {
        let batch: Vec<T> = request(subject, &page).await?;
        let full = batch.len() as u32 >= PAGE;

        page.after = batch.last().map(&key).or(page.after);
        rows.extend(batch);

        if !full {
            return Ok(rows);
        }
    }
}

async fn query(ctx: &SessionContext, sql: &str) -> Result<Vec<RecordBatch>> {
    let frame = ctx
        .sql(sql)
        .await
        .with_context(|| format!("planning {sql}"))?;
    Ok(frame.collect().await?)
}

/// A column of a result as strings, whatever string type it came as.
fn strings(batch: &RecordBatch, column: usize) -> Result<StringArray> {
    let array = cast(batch.column(column), &DataType::Utf8)?;
    Ok(array
        .as_any()
        .downcast_ref::<StringArray>()
        .context("a string column")?
        .clone())
}

/// A column of a result as 64-bit integers.
fn integers(batch: &RecordBatch, column: usize) -> Result<Int64Array> {
    let array = cast(batch.column(column), &DataType::Int64)?;
    Ok(array
        .as_any()
        .downcast_ref::<Int64Array>()
        .context("an integer column")?
        .clone())
}

/// A clause keeping rows logged from cutover on, or keeping none before it.
fn since_cutover(native: Option<&str>) -> &str {
    native.unwrap_or("FALSE")
}

async fn count_views(
    ctx: &SessionContext,
    tallies: &mut Tallies,
    native: Option<&str>,
) -> Result<()> {
    let mut legacy =
        sqlx::query!(r#"SELECT "project_id", "day", "views" FROM "orrery_project_views""#)
            .fetch(db());

    while let Some(row) = legacy.try_next().await? {
        if let Some(project) = tallies.project(&row.project_id) {
            tallies.count(
                Series::Measure(Measure::Views),
                project,
                &row.day,
                row.views,
            );
        }
    }

    let Some(native) = native else {
        return Ok(());
    };

    let by_day = query(
        ctx,
        &format!(
            r#"SELECT "path", substr("when", 1, 10), COUNT(*)
               FROM page_views_realtime
               WHERE {native}
               GROUP BY "path", substr("when", 1, 10)"#
        ),
    )
    .await?;

    for batch in &by_day {
        let (paths, days, counts) = (strings(batch, 0)?, strings(batch, 1)?, integers(batch, 2)?);
        for row in 0..batch.num_rows() {
            if let Some(project) = tallies.project_at(paths.value(row)) {
                tallies.count(
                    Series::Measure(Measure::Views),
                    project,
                    days.value(row),
                    counts.value(row),
                );
            }
        }
    }

    let by_viewer = query(
        ctx,
        &format!(
            r#"SELECT "path", "user_id", COUNT(*)
               FROM page_views_realtime
               WHERE "user_id" IS NOT NULL AND {native}
               GROUP BY "path", "user_id""#
        ),
    )
    .await?;

    for batch in &by_viewer {
        let (paths, users, counts) = (strings(batch, 0)?, strings(batch, 1)?, integers(batch, 2)?);
        for row in 0..batch.num_rows() {
            let Some(project) = tallies.project_at(paths.value(row)) else {
                continue;
            };
            let Ok(user) = users.value(row).parse::<Ulid>() else {
                continue;
            };
            *tallies.viewers.entry((project, user)).or_default() += counts.value(row);
        }
    }

    Ok(())
}

async fn count_visits(
    ctx: &SessionContext,
    tallies: &mut Tallies,
    native: Option<&str>,
) -> Result<()> {
    let since = since_cutover(native);
    let clicks = format!(
        r#"("link_type" = '{ORRERY_VISIT_LINK_TYPE}'
            OR ("link_type" = '{VISIT_LINK_TYPE}' AND {since}))"#
    );
    // Orrery's own exchange visits are already among its visits; the ones
    // synced into the exchange log are told apart by their ids, which carry
    // no time.
    let exchange = format!(
        r#""action" = 'visit' AND "page_id" IS NOT NULL
           AND "event_id" NOT LIKE '0000000000%' AND {since}"#
    );

    let by_day = query(
        ctx,
        &format!(
            r#"SELECT "from_path", substr("when", 1, 10), COUNT(*)
               FROM clickthroughs_realtime
               WHERE {clicks}
               GROUP BY "from_path", substr("when", 1, 10)"#
        ),
    )
    .await?;

    for batch in &by_day {
        let (paths, days, counts) = (strings(batch, 0)?, strings(batch, 1)?, integers(batch, 2)?);
        for row in 0..batch.num_rows() {
            if let Some(project) = tallies.project_at(paths.value(row)) {
                tallies.count(
                    Series::Measure(Measure::Visits),
                    project,
                    days.value(row),
                    counts.value(row),
                );
            }
        }
    }

    let visitors = query(
        ctx,
        &format!(
            r#"SELECT DISTINCT "from_path", "user_id"
               FROM clickthroughs_realtime
               WHERE "user_id" IS NOT NULL AND {clicks}"#
        ),
    )
    .await?;

    for batch in &visitors {
        let (paths, users) = (strings(batch, 0)?, strings(batch, 1)?);
        for row in 0..batch.num_rows() {
            if let (Some(project), Ok(user)) = (
                tallies.project_at(paths.value(row)),
                users.value(row).parse::<Ulid>(),
            ) {
                tallies.visitors.insert((project, user));
            }
        }
    }

    if native.is_none() {
        return Ok(());
    }

    let by_day = query(
        ctx,
        &format!(
            r#"SELECT "page_id", substr("when", 1, 10), COUNT(*)
               FROM exchange_events_realtime
               WHERE {exchange}
               GROUP BY "page_id", substr("when", 1, 10)"#
        ),
    )
    .await?;

    for batch in &by_day {
        let (pages, days, counts) = (strings(batch, 0)?, strings(batch, 1)?, integers(batch, 2)?);
        for row in 0..batch.num_rows() {
            if let Some(project) = tallies.project(pages.value(row)) {
                tallies.count(
                    Series::Measure(Measure::Visits),
                    project,
                    days.value(row),
                    counts.value(row),
                );
            }
        }
    }

    let visitors = query(
        ctx,
        &format!(
            r#"SELECT DISTINCT "page_id", "user_id"
               FROM exchange_events_realtime
               WHERE "user_id" IS NOT NULL AND {exchange}"#
        ),
    )
    .await?;

    for batch in &visitors {
        let (pages, users) = (strings(batch, 0)?, strings(batch, 1)?);
        for row in 0..batch.num_rows() {
            if let (Some(project), Ok(user)) = (
                tallies.project(pages.value(row)),
                users.value(row).parse::<Ulid>(),
            ) {
                tallies.visitors.insert((project, user));
            }
        }
    }

    Ok(())
}

async fn count_shares(
    ctx: &SessionContext,
    tallies: &mut Tallies,
    native: Option<&str>,
) -> Result<()> {
    let legacy = sqlx::query!(
        r#"SELECT "entity_id", "platform", substr("when", 1, 10) AS "day!: String",
                  COUNT(*) AS "count!: i64"
           FROM "orrery_shares"
           GROUP BY "entity_id", "platform", substr("when", 1, 10)"#
    )
    .fetch_all(db())
    .await?;

    for row in legacy {
        if let Some(project) = tallies.project(&row.entity_id) {
            let platform = row.platform.to_lowercase();
            tallies.count(Series::Share(platform), project, &row.day, row.count);
        }
    }

    let Some(native) = native else {
        return Ok(());
    };

    let by_day = query(
        ctx,
        &format!(
            r#"SELECT "target_id", "platform", substr("when", 1, 10), COUNT(*)
               FROM share_events_realtime
               WHERE {native}
               GROUP BY "target_id", "platform", substr("when", 1, 10)"#
        ),
    )
    .await?;

    for batch in &by_day {
        let (targets, platforms, days, counts) = (
            strings(batch, 0)?,
            strings(batch, 1)?,
            strings(batch, 2)?,
            integers(batch, 3)?,
        );
        for row in 0..batch.num_rows() {
            if let Some(project) = tallies.project(targets.value(row)) {
                tallies.count(
                    Series::Share(platforms.value(row).to_lowercase()),
                    project,
                    days.value(row),
                    counts.value(row),
                );
            }
        }
    }

    Ok(())
}

async fn count_contributions(ctx: &SessionContext, tallies: &mut Tallies) -> Result<()> {
    let joins = ParticipationRow::JOIN_TASK_NAMES
        .map(|task| format!("'{}'", task.replace('\'', "''")))
        .join(", ");

    let by_day = query(
        ctx,
        &format!(
            r#"SELECT "entity_id", substr("when", 1, 10), COUNT(*)
               FROM participation_realtime
               WHERE "task_name" IS NULL OR "task_name" NOT IN ({joins})
               GROUP BY "entity_id", substr("when", 1, 10)"#
        ),
    )
    .await?;

    for batch in &by_day {
        let (entities, days, counts) =
            (strings(batch, 0)?, strings(batch, 1)?, integers(batch, 2)?);
        for row in 0..batch.num_rows() {
            if let Some(project) = tallies.project(entities.value(row)) {
                tallies.count(
                    Series::Measure(Measure::Contributions),
                    project,
                    days.value(row),
                    counts.value(row),
                );
            }
        }
    }

    let through = &tallies.through;
    let by_participant = query(
        ctx,
        &format!(
            r#"SELECT "entity_id", "user_id", COUNT(*), MAX("when")
               FROM participation_realtime
               WHERE "user_id" IS NOT NULL
                 AND ("task_name" IS NULL OR "task_name" NOT IN ({joins}))
                 AND substr("when", 1, 10) <= '{through}'
               GROUP BY "entity_id", "user_id""#
        ),
    )
    .await?;

    for batch in &by_participant {
        let (entities, users, counts, lasts) = (
            strings(batch, 0)?,
            strings(batch, 1)?,
            integers(batch, 2)?,
            strings(batch, 3)?,
        );
        for row in 0..batch.num_rows() {
            let Some(project) = tallies.project(entities.value(row)) else {
                continue;
            };
            let Ok(user) = users.value(row).parse::<Ulid>() else {
                continue;
            };
            tallies.participants.push((
                project,
                user,
                counts.value(row),
                lasts.value(row).to_string(),
            ));
        }
    }

    Ok(())
}

async fn count_saves(tallies: &mut Tallies) -> Result<()> {
    let saves =
        paged::<(Ulid, Ulid), Save>("user.stats.saves", |save| (save.user_id, save.project_id))
            .await
            .context("reading saves")?;

    for save in saves {
        if tallies.published.contains_key(&save.project_id) {
            tallies.count(
                Series::Measure(Measure::Saves),
                save.project_id,
                &save.day,
                1,
            );
        }
    }

    Ok(())
}

async fn count_joins(tallies: &mut Tallies) -> Result<()> {
    let joins =
        paged::<(Ulid, Ulid), JoinPair>("user.stats.joins", |join| (join.user_id, join.project_id))
            .await
            .context("reading joins")?;

    for join in joins {
        if tallies.published.contains_key(&join.project_id) {
            tallies.count(
                Series::Measure(Measure::Joins),
                join.project_id,
                &join.when,
                1,
            );
            tallies.joiners.insert((join.project_id, join.user_id));
        }
    }

    Ok(())
}

/// Replace the snapshot with `tallies`.
async fn write(tallies: &Tallies, now: Timestamp) -> Result<()> {
    let rows = Rows::of(tallies)?;

    let mut tx = Immediate::begin_with_retry(db(), None, None).await?;

    for clear in [
        r#"DELETE FROM "project_stats_projects""#,
        r#"DELETE FROM "project_stats_totals""#,
        r#"DELETE FROM "project_stats_days""#,
        r#"DELETE FROM "project_stats_distributions""#,
        r#"DELETE FROM "project_stats_feedback""#,
        r#"DELETE FROM "project_stats_viewers""#,
        r#"DELETE FROM "project_stats_people""#,
        r#"DELETE FROM "project_stats_participants""#,
    ] {
        sqlx::query(clear).execute(&mut tx).await?;
    }

    insert(
        &mut tx,
        r#"INSERT INTO "project_stats_projects" ("project_id", "published") "#,
        rows.projects,
        |mut row, (project, published)| {
            row.push_bind(project).push_bind(published);
        },
    )
    .await?;

    insert(
        &mut tx,
        r#"INSERT INTO "project_stats_totals" ("project_id", "series", "span", "total") "#,
        rows.totals,
        |mut row, (project, series, span, total)| {
            row.push_bind(project)
                .push_bind(series)
                .push_bind(span)
                .push_bind(total);
        },
    )
    .await?;

    insert(
        &mut tx,
        r#"INSERT INTO "project_stats_days"
           ("project_id", "day", "views", "visits", "saves", "contributions") "#,
        rows.days,
        |mut row, (project, counts)| {
            row.push_bind(project)
                .push_bind(counts.day)
                .push_bind(counts.views)
                .push_bind(counts.visits)
                .push_bind(counts.saves)
                .push_bind(counts.contributions);
        },
    )
    .await?;

    insert(
        &mut tx,
        r#"INSERT INTO "project_stats_distributions" ("series", "span", "sorted_values") "#,
        rows.distributions,
        |mut row, (series, span, values)| {
            row.push_bind(series).push_bind(span).push_bind(values);
        },
    )
    .await?;

    insert(
        &mut tx,
        r#"INSERT INTO "project_stats_feedback" ("project_id", "joined", "unknown") "#,
        rows.feedback,
        |mut row, (project, joined, unknown)| {
            row.push_bind(project).push_bind(joined).push_bind(unknown);
        },
    )
    .await?;

    insert(
        &mut tx,
        r#"INSERT INTO "project_stats_viewers" ("project_id", "user_id", "views") "#,
        rows.viewers,
        |mut row, (project, user, views)| {
            row.push_bind(project).push_bind(user).push_bind(views);
        },
    )
    .await?;

    insert(
        &mut tx,
        r#"INSERT INTO "project_stats_people" ("user_id", "gender", "birthday") "#,
        rows.people,
        |mut row, (user, gender, birthday)| {
            row.push_bind(user).push_bind(gender).push_bind(birthday);
        },
    )
    .await?;

    insert(
        &mut tx,
        r#"INSERT INTO "project_stats_participants"
           ("project_id", "user_id", "contributions", "last_contribution") "#,
        rows.participants,
        |mut row, (project, user, contributions, last)| {
            row.push_bind(project)
                .push_bind(user)
                .push_bind(contributions)
                .push_bind(last);
        },
    )
    .await?;

    let built = now.to_string();
    sqlx::query!(
        r#"INSERT INTO "project_stats_snapshot" ("id", "built", "through")
           VALUES (1, ?, ?)
           ON CONFLICT ("id") DO UPDATE SET "built" = excluded."built",
                                            "through" = excluded."through""#,
        built,
        tallies.through,
    )
    .execute(&mut tx)
    .await?;

    tx.commit().await?;

    Ok(())
}

/// The rows of each snapshot table, as they are written.
struct Rows {
    projects: Vec<(String, bool)>,
    totals: Vec<(String, String, &'static str, i64)>,
    days: Vec<(String, DayCounts)>,
    distributions: Vec<(String, &'static str, String)>,
    feedback: Vec<(String, i64, i64)>,
    viewers: Vec<(String, String, i64)>,
    people: Vec<(String, &'static str, Option<String>)>,
    participants: Vec<(String, String, i64, String)>,
}

impl Rows {
    fn of(tallies: &Tallies) -> Result<Rows> {
        let feedback = tallies.feedback();
        let staff: HashSet<Ulid> = tallies
            .people
            .iter()
            .filter(|person| person.is_staff)
            .map(|person| person.user_id)
            .collect();

        let mut totals = Vec::new();
        for ((series, project), tally) in &tallies.totals {
            for window in WINDOWS {
                let total = tally.get(window);
                if total > 0 {
                    totals.push((project.to_string(), series.key(), window.key(), total));
                }
            }
        }

        let mut distributions = Vec::new();
        for ((series, span), values) in tallies.distributions(&feedback) {
            distributions.push((series, span, serde_json::to_string(&values)?));
        }

        Ok(Rows {
            projects: tallies
                .published
                .iter()
                .map(|(project, published)| (project.to_string(), *published))
                .collect(),
            totals,
            days: tallies
                .days
                .iter()
                .map(|((project, _), counts)| (project.to_string(), counts.clone()))
                .collect(),
            distributions,
            feedback: feedback
                .into_iter()
                .map(|(project, (joined, unknown))| (project.to_string(), joined, unknown))
                .collect(),
            viewers: tallies
                .viewers
                .iter()
                .filter(|((_, user), _)| !staff.contains(user) && !tallies.forgotten.contains(user))
                .map(|((project, user), views)| (project.to_string(), user.to_string(), *views))
                .collect(),
            people: tallies
                .people
                .iter()
                .filter(|person| !person.is_staff)
                .filter(|person| person.gender != Gender::Unknown || person.birthday.is_some())
                .map(|person| {
                    (
                        person.user_id.to_string(),
                        person.gender.key(),
                        person.birthday.clone(),
                    )
                })
                .collect(),
            participants: tallies
                .participants
                .iter()
                .filter(|(_, user, _, _)| !tallies.forgotten.contains(user))
                .map(|(project, user, contributions, last)| {
                    (
                        project.to_string(),
                        user.to_string(),
                        *contributions,
                        last.clone(),
                    )
                })
                .collect(),
        })
    }
}

/// Insert `rows` with `head`, the INSERT up to its VALUES, in batches.
async fn insert<T>(
    tx: &mut Immediate,
    head: &'static str,
    rows: Vec<T>,
    bind: impl Fn(sqlx::query_builder::Separated<'_, sqlx::Sqlite, &'static str>, T),
) -> Result<()> {
    let mut rows = rows.into_iter().peekable();

    while rows.peek().is_some() {
        let mut builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(head);
        builder.push_values(rows.by_ref().take(BATCH), &bind);
        builder.build().execute(&mut *tx).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    const BEES: &str = "01HQ3Z8Y6V2N4J5K7M8P9R0P01";
    const MOTHS: &str = "01HQ3Z8Y6V2N4J5K7M8P9R0P02";
    const ADA: &str = "01HQ3Z8Y6V2N4J5K7M8P9R0S1A";
    const BEA: &str = "01HQ3Z8Y6V2N4J5K7M8P9R0S1B";

    fn tallies() -> Tallies {
        let project = |id: &str, slug: &str| ProjectSlugs {
            project_id: id.parse().unwrap(),
            published: true,
            slugs: vec![slug.to_string()],
        };

        Tallies::new(
            Date::constant(2026, 11, 16),
            vec![project(BEES, "bee-count"), project(MOTHS, "moth-night")],
        )
        .unwrap()
    }

    #[test]
    fn a_count_lands_in_each_window_it_falls_in() {
        let mut tallies = tallies();
        let bees: Ulid = BEES.parse().unwrap();
        let views = Series::Measure(Measure::Views);

        tallies.count(views.clone(), bees, "2026-11-16T23:00:00Z", 1);
        tallies.count(views.clone(), bees, "2026-11-01", 10);
        tallies.count(views.clone(), bees, "2025-01-01", 100);
        tallies.count(views.clone(), bees, "2026-11-17", 1_000);

        check!(
            tallies.totals[&(views, bees)]
                == Tally {
                    all_time: 111,
                    last_30: 11,
                    last_7: 1,
                }
        );
        check!(tallies.days.len() == 2);
    }

    #[test]
    fn a_path_finds_its_project_by_any_slug() {
        let tallies = tallies();

        check!(tallies.project_at("/projects/bee-count") == Some(BEES.parse().unwrap()));
        check!(tallies.project_at("/Moth-Night") == Some(MOTHS.parse().unwrap()));
        check!(tallies.project_at("/projects/bee-count/stats") == None);
        check!(tallies.project_at("/about") == None);
    }

    #[test]
    fn feedback_counts_visitors_who_have_not_joined() {
        let mut tallies = tallies();
        let bees: Ulid = BEES.parse().unwrap();
        let (ada, bea): (Ulid, Ulid) = (ADA.parse().unwrap(), BEA.parse().unwrap());

        tallies.joiners.insert((bees, ada));
        tallies.visitors.insert((bees, ada));
        tallies.visitors.insert((bees, bea));

        check!(tallies.feedback()[&bees] == (1, 1));

        let distributions = tallies.distributions(&tallies.feedback());
        check!(distributions[&(FEEDBACK_JOINS_PERCENT.to_string(), "all_time")] == [50.0]);
    }

    #[test]
    fn a_distribution_holds_only_projects_with_any() {
        let mut tallies = tallies();
        let saves = Series::Measure(Measure::Saves);

        tallies.count(saves.clone(), BEES.parse().unwrap(), "2026-11-16", 3);
        tallies.count(saves.clone(), MOTHS.parse().unwrap(), "2020-11-16", 1);

        let distributions = tallies.distributions(&BTreeMap::new());
        check!(distributions[&(saves.key(), "all_time")] == [1.0, 3.0]);
        check!(distributions[&(saves.key(), "last_30")] == [3.0]);
    }

    #[test]
    fn a_deleted_account_is_named_by_no_row_though_its_logs_count() {
        let mut tallies = tallies();
        let bees: Ulid = BEES.parse().unwrap();
        let (ada, bea): (Ulid, Ulid) = (ADA.parse().unwrap(), BEA.parse().unwrap());

        for user in [ada, bea] {
            tallies.viewers.insert((bees, user), 2);
            tallies
                .participants
                .push((bees, user, 3, "2026-11-16T00:00:00Z".to_string()));
        }
        tallies.forgotten.insert(ada);

        let rows = Rows::of(&tallies).unwrap();
        check!(rows.viewers == [(BEES.to_string(), BEA.to_string(), 2)]);
        check!(rows.participants.len() == 1);
        check!(rows.participants[0].1 == BEA);
    }
}
