//! `/projects/<slug>/stats`: how a project is doing, for the people who may
//! edit it and for SciStarter's staff, laid out as orrery's Stats tab was.
//!
//! The numbers come from the log service's daily snapshot
//! (`log.project_stats`); this module turns them into what the page draws.
//! Every chart is drawn here as SVG, so the page loads no charting script.

pub mod slugs;

use anyhow::Error;
use common::{
    message::{
        dashboard::EntitySummary,
        project_stats::{
            Comparison, DayCounts, Measure, ProjectStats, ProjectStatsRequest,
            ProjectStatsResponse, ShareStats, Window,
        },
        user::BasicInfo,
    },
    service::request,
    shared::db,
};
use jiff::civil::Date;
use poem::{IntoResponse, Response, handler, http::StatusCode, web::Path};
use sailfish::TemplateOnce;
use std::{collections::HashMap, sync::Arc};
use ulid::Ulid;

use crate::{
    component::{Component, Head, StandardLayout},
    dashboard::entity_summary,
    entity::EntityKind,
    middleware::authorization::Authenticated,
    number::thousands,
    project::editor_nav::{EditorNav, EditorPage, Gate, gate},
};

/// The path of a project's stats page.
pub fn path(slug: &str) -> String {
    path_from_page(&EntityKind::Project.path(slug))
}

/// The path of the stats page of the project whose page is at `page`.
pub fn path_from_page(project_page: &str) -> String {
    format!("{project_page}/stats")
}

#[handler]
pub async fn page(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<Response, Error> {
    let here = path(&slug);

    let (user_info, project_id) = match gate(&slug, &here, user_info).await? {
        Gate::Open { user, project_id } => (user, project_id),
        Gate::Closed(response) => return Ok(response),
    };

    let project = sqlx::query!(
        r#"SELECT "name", "is_affiliate" AS "is_affiliate: bool"
           FROM "projects" WHERE "project_id" = ?"#,
        project_id.to_string(),
    )
    .fetch_optional(db())
    .await?;

    let Some(project) = project else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let response: ProjectStatsResponse =
        request("log.project_stats", ProjectStatsRequest { project_id }).await?;

    let stats = match response {
        ProjectStatsResponse::Ready(stats) => Some(StatsView::of(*stats).await?),
        ProjectStatsResponse::Pending => None,
    };

    let content = StatsContent {
        nav: EditorNav::new(&slug, project_id, EditorPage::Stats),
        name: project.name.clone(),
        project_path: EntityKind::Project.path(&slug),
        is_affiliate: project.is_affiliate,
        stats,
    };

    render(here, project.name, content, user_info).await
}

async fn render(
    here: String,
    name: String,
    content: StatsContent,
    user_info: Arc<BasicInfo>,
) -> Result<Response, Error> {
    let layout = StandardLayout::new(here, content)
        .prepare(Some(user_info))
        .await?;

    StatsPage {
        head: Head::default().prepare(None).await?,
        title: format!("{name} stats"),
        layout,
    }
    .into_response(None)
    .await
}

#[derive(TemplateOnce)]
#[template(path = "pages/project_stats.stpl.html")]
struct StatsPage {
    head: <Head as Component>::Prepared,
    title: String,
    layout: <StandardLayout<StatsContent> as Component>::Prepared,
}

#[derive(TemplateOnce)]
#[template(path = "pages/project_stats_content.stpl.html")]
struct StatsContent {
    nav: EditorNav,
    name: String,
    project_path: String,
    is_affiliate: bool,
    /// `None` until the first snapshot has been built.
    stats: Option<StatsView>,
}

/// Everything the page draws.
struct StatsView {
    /// The last day counted, as the page names it.
    through: String,
    all_time: Vec<Card>,
    last_30: Vec<Card>,
    conversions_30: DayChart,
    conversions_90: DayChart,
    shares: Vec<SharesView>,
    audience_views: String,
    by_gender: Vec<BarRow>,
    by_age: Vec<BarRow>,
    also_viewed: Vec<AlsoViewed>,
    feedback: FeedbackView,
    contributions: Vec<Card>,
    contributions_30: DayChart,
    contributions_90: DayChart,
}

impl StatsView {
    async fn of(stats: ProjectStats) -> Result<StatsView, Error> {
        let card = |measure: Measure, window: Window| {
            let comparison = stats
                .comparison(measure, window)
                .copied()
                .unwrap_or_default();
            Card::new(measure, window, comparison)
        };

        let events = [
            Measure::Views,
            Measure::Visits,
            Measure::Saves,
            Measure::Joins,
        ];
        let last = |days: usize| &stats.days[stats.days.len().saturating_sub(days)..];

        Ok(StatsView {
            through: day_label(&stats.through, true),
            all_time: events.map(|m| card(m, Window::AllTime)).to_vec(),
            last_30: events.map(|m| card(m, Window::Last30)).to_vec(),
            conversions_30: DayChart::conversions(last(30)),
            conversions_90: DayChart::conversions(last(90)),
            shares: [Window::AllTime, Window::Last30]
                .into_iter()
                .filter_map(|window| stats.shares(window))
                .map(SharesView::of)
                .collect(),
            audience_views: thousands(stats.audience.views),
            by_gender: BarRow::rows(
                stats
                    .audience
                    .by_gender
                    .iter()
                    .map(|(gender, views)| (gender.label(), *views)),
            ),
            by_age: BarRow::rows(
                stats
                    .audience
                    .by_age
                    .iter()
                    .map(|(age, views)| (age.label(), *views)),
            ),
            also_viewed: AlsoViewed::of(&stats.audience.also_viewed).await?,
            feedback: FeedbackView::of(&stats),
            contributions: [Window::AllTime, Window::Last7, Window::Last30]
                .map(|window| card(Measure::Contributions, window))
                .to_vec(),
            contributions_30: DayChart::contributions(last(30)),
            contributions_90: DayChart::contributions(last(90)),
        })
    }
}

/// One measure over one window, beside every project's.
#[derive(Clone)]
struct Card {
    icon: &'static str,
    label: &'static str,
    description: &'static str,
    timeframe: &'static str,
    figure: String,
    median: MedAvg,
    mean: MedAvg,
}

impl Card {
    fn new(measure: Measure, window: Window, comparison: Comparison) -> Card {
        let (icon, label, description) = match (measure, window) {
            (Measure::Views, _) => (
                "db-views",
                "Views",
                "How many times your project's page on SciStarter was viewed",
            ),
            (Measure::Visits, _) => (
                "org-visits",
                "Visits",
                "A visit is when someone clicks from your SciStarter project page to your website",
            ),
            (Measure::Saves, _) => (
                "db-bookmark",
                "Saves",
                "A save is when someone saves your project for later, which puts it in the saves area of their dashboard",
            ),
            (Measure::Joins, _) => (
                "org-joins",
                "Joins",
                "A join is when someone marks in their SciStarter dashboard that they have joined your project, or when your project confirms their membership by reporting their participation",
            ),
            (Measure::Contributions, Window::AllTime) => {
                ("org-contributions", "All-Time Contributions", "")
            }
            (Measure::Contributions, Window::Last7) => ("week", "Last Week's Contributions", ""),
            (Measure::Contributions, Window::Last30) => {
                ("month", "Last 30 Days' Contributions", "")
            }
        };

        Card {
            icon,
            label,
            description,
            timeframe: match window {
                Window::AllTime => "All-Time",
                Window::Last30 => "30 Days",
                Window::Last7 => "7 Days",
            },
            figure: thousands(comparison.value as i64),
            median: MedAvg::median("Median across all projects", &comparison),
            mean: MedAvg::mean("Mean across all projects", &comparison),
        }
    }
}

/// A figure placed on a line beside every project's median or mean, as
/// orrery's comparison graph placed it.
#[derive(Clone)]
struct MedAvg {
    label: String,
    figure: String,
    /// Where the projects' median or mean falls, as a percentage of the
    /// line.
    midline: f64,
    /// Where this project falls, likewise.
    dot: f64,
    /// This project's figure, as the dot's tooltip gives it.
    project: String,
}

impl MedAvg {
    /// Against the median: the median sits mid-line and the project at its
    /// percentile.
    fn median(label: &str, comparison: &Comparison) -> MedAvg {
        MedAvg {
            label: label.to_string(),
            figure: format!("{:.1}", comparison.median),
            midline: 50.0,
            dot: comparison.percentile.clamp(0.0, 100.0),
            project: figure(comparison.value),
        }
    }

    /// Against the mean: both sit where they fall between zero and the
    /// largest project's figure.
    fn mean(label: &str, comparison: &Comparison) -> MedAvg {
        let along = |value: f64| {
            if comparison.max > 0.0 {
                (100.0 * value / comparison.max).clamp(0.0, 100.0)
            } else {
                0.0
            }
        };

        MedAvg {
            label: label.to_string(),
            figure: format!("{:.1}", comparison.mean),
            midline: along(comparison.mean),
            dot: along(comparison.value),
            project: figure(comparison.value),
        }
    }
}

/// A figure as the page shows it: whole numbers grouped, fractions to one
/// place.
fn figure(value: f64) -> String {
    if value.fract() == 0.0 {
        thousands(value as i64)
    } else {
        format!("{value:.1}")
    }
}

/// A `YYYY-MM-DD` day as the page names it: `Nov 16`, or `Nov 16, 2026`
/// with the year.
fn day_label(day: &str, with_year: bool) -> String {
    match day.parse::<Date>() {
        Ok(date) if with_year => date.strftime("%b %-d, %Y").to_string(),
        Ok(date) => date.strftime("%b %-d").to_string(),
        Err(_) => day.to_string(),
    }
}

/// Counts by day, drawn as bars with lines over them on one scale. The
/// chart's own units are 10 across per day and 100 up, stretched to fit.
struct DayChart {
    width: usize,
    bars: Vec<ChartBar>,
    lines: Vec<ChartLine>,
    /// The largest count, which the top of the chart stands for.
    top: String,
    first: String,
    middle: String,
    last: String,
}

struct ChartBar {
    x: f64,
    y: f64,
    height: f64,
    title: String,
}

struct ChartLine {
    class: &'static str,
    points: String,
}

const CHART_HEIGHT: f64 = 100.0;
const DAY_WIDTH: f64 = 10.0;

impl DayChart {
    /// Views as bars, and visits and saves as lines over them, as orrery's
    /// conversion chart drew them.
    fn conversions(days: &[DayCounts]) -> DayChart {
        DayChart::new(
            days,
            |day| day.views,
            &[
                ("ps-line-visits", |day| day.visits),
                ("ps-line-saves", |day| day.saves),
            ],
            |day| {
                format!(
                    "{}: {} views, {} visits, {} saves",
                    day_label(&day.day, false),
                    thousands(day.views),
                    thousands(day.visits),
                    thousands(day.saves)
                )
            },
        )
    }

    fn contributions(days: &[DayCounts]) -> DayChart {
        DayChart::new(
            days,
            |day| day.contributions,
            &[],
            |day| {
                format!(
                    "{}: {} contributions",
                    day_label(&day.day, false),
                    thousands(day.contributions)
                )
            },
        )
    }

    fn new(
        days: &[DayCounts],
        bar: fn(&DayCounts) -> i64,
        lines: &[(&'static str, fn(&DayCounts) -> i64)],
        title: impl Fn(&DayCounts) -> String,
    ) -> DayChart {
        let top = days
            .iter()
            .flat_map(|day| {
                std::iter::once(bar(day)).chain(lines.iter().map(|(_, line)| line(day)))
            })
            .max()
            .unwrap_or(0);
        let scale = |count: i64| {
            if top > 0 {
                CHART_HEIGHT * count as f64 / top as f64
            } else {
                0.0
            }
        };

        let bars = days
            .iter()
            .enumerate()
            .map(|(i, day)| {
                let height = scale(bar(day));
                ChartBar {
                    x: i as f64 * DAY_WIDTH + 1.0,
                    y: CHART_HEIGHT - height,
                    height,
                    title: title(day),
                }
            })
            .collect();

        let lines = lines
            .iter()
            .map(|(class, line)| ChartLine {
                class,
                points: days
                    .iter()
                    .enumerate()
                    .map(|(i, day)| {
                        format!(
                            "{:.1},{:.1}",
                            i as f64 * DAY_WIDTH + DAY_WIDTH / 2.0,
                            CHART_HEIGHT - scale(line(day))
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            })
            .collect();

        let label = |i: usize| {
            days.get(i)
                .map(|day| day_label(&day.day, false))
                .unwrap_or_default()
        };

        DayChart {
            width: days.len() * DAY_WIDTH as usize,
            bars,
            lines,
            top: thousands(top),
            first: label(0),
            middle: label(days.len() / 2),
            last: label(days.len().saturating_sub(1)),
        }
    }
}

/// A ring divided among a few figures, drawn as stroked circles whose
/// circumference is 100.
struct Donut {
    segments: Vec<DonutSegment>,
}

struct DonutSegment {
    class: &'static str,
    /// The segment's share of the ring, 0 to 100.
    share: f64,
    /// Where it starts, as a stroke-dashoffset from the top of the ring.
    offset: f64,
}

impl Donut {
    fn of(parts: &[(&'static str, i64)]) -> Donut {
        let total: i64 = parts.iter().map(|(_, count)| count).sum();
        let mut start = 0.0;

        let segments = parts
            .iter()
            .filter(|(_, count)| *count > 0 && total > 0)
            .map(|(class, count)| {
                let share = 100.0 * *count as f64 / total as f64;
                let segment = DonutSegment {
                    class,
                    share,
                    offset: 25.0 - start,
                };
                start += share;
                segment
            })
            .collect();

        Donut { segments }
    }
}

/// A platform the share log names, as the page shows it.
fn platform(name: &str) -> (&'static str, &'static str) {
    match name {
        "facebook" => ("Facebook", "ps-share-facebook"),
        "twitter" => ("Twitter", "ps-share-twitter"),
        "pinterest" => ("Pinterest", "ps-share-pinterest"),
        "linkedin" => ("LinkedIn", "ps-share-linkedin"),
        "instagram" => ("Instagram", "ps-share-instagram"),
        _ => ("Other", "ps-share-other"),
    }
}

/// Shares over one window.
struct SharesView {
    id: &'static str,
    timeframe: &'static str,
    total: String,
    donut: Donut,
    platforms: Vec<SharePlatformView>,
}

struct SharePlatformView {
    label: &'static str,
    class: &'static str,
    count: String,
    median: MedAvg,
    mean: MedAvg,
}

impl SharesView {
    fn of(shares: &ShareStats) -> SharesView {
        let platforms: Vec<(&'static str, &'static str, &Comparison)> = shares
            .platforms
            .iter()
            .map(|(name, comparison)| {
                let (label, class) = platform(name);
                (label, class, comparison)
            })
            .collect();

        SharesView {
            id: match shares.window {
                Window::AllTime => "shares-all-time",
                Window::Last30 => "shares-last-30",
                Window::Last7 => "shares-last-7",
            },
            timeframe: match shares.window {
                Window::AllTime => "All-Time",
                Window::Last30 => "Last 30 Days",
                Window::Last7 => "Last 7 Days",
            },
            total: thousands(shares.total),
            donut: Donut::of(
                &platforms
                    .iter()
                    .map(|(_, class, comparison)| (*class, comparison.value as i64))
                    .collect::<Vec<_>>(),
            ),
            platforms: platforms
                .into_iter()
                .map(|(label, class, comparison)| SharePlatformView {
                    label,
                    class,
                    count: figure(comparison.value),
                    median: MedAvg::median(&format!("{label} Median"), comparison),
                    mean: MedAvg::mean(&format!("{label} Average"), comparison),
                })
                .collect(),
        }
    }
}

/// One bar of a horizontal bar chart.
struct BarRow {
    label: &'static str,
    count: String,
    /// The bar's length, as a percentage of the longest.
    width: f64,
}

impl BarRow {
    fn rows(counts: impl Iterator<Item = (&'static str, i64)>) -> Vec<BarRow> {
        let counts: Vec<(&'static str, i64)> = counts.collect();
        let top = counts.iter().map(|(_, count)| *count).max().unwrap_or(0);

        counts
            .into_iter()
            .map(|(label, count)| BarRow {
                label,
                count: thousands(count),
                width: if top > 0 {
                    100.0 * count as f64 / top as f64
                } else {
                    0.0
                },
            })
            .collect()
    }
}

/// A project the viewers of this one also view.
struct AlsoViewed {
    name: String,
    link: String,
    image: Option<String>,
    goal: String,
    views: String,
}

impl AlsoViewed {
    async fn of(projects: &[(Ulid, i64)]) -> Result<Vec<AlsoViewed>, Error> {
        let ids: Vec<String> = projects.iter().map(|(id, _)| id.to_string()).collect();
        let summaries: HashMap<Ulid, EntitySummary> = entity_summary::lookup_batch(&ids)
            .await?
            .into_iter()
            .map(|summary| (summary.id, summary))
            .collect();
        let goals = goals(&ids).await?;

        // In the order the log service ranked them, most viewed first.
        Ok(projects
            .iter()
            .filter_map(|(id, views)| {
                let summary = summaries.get(id)?;
                Some(AlsoViewed {
                    name: summary.name.clone(),
                    link: entity_summary::entity_link(summary),
                    image: summary.image.clone(),
                    goal: goals.get(id).cloned().unwrap_or_default(),
                    views: thousands(*views),
                })
            })
            .collect())
    }
}

/// The goal of each of the projects `ids` names.
async fn goals(ids: &[String]) -> Result<HashMap<Ulid, String>, Error> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        r#"SELECT "project_id", "goal" FROM "projects" WHERE "project_id" IN ("#,
    );
    let mut list = query.separated(", ");
    for id in ids {
        list.push_bind(id);
    }
    query.push(")");

    let rows: Vec<(String, String)> = query.build_query_as().fetch_all(db()).await?;

    Ok(rows
        .into_iter()
        .filter_map(|(id, goal)| Some((id.parse().ok()?, goal)))
        .collect())
}

/// Joins against the visitors who have not said whether they joined.
struct FeedbackView {
    joins: String,
    unknown: String,
    pie: Donut,
    rows: Vec<[MedAvg; 4]>,
}

impl FeedbackView {
    fn of(stats: &ProjectStats) -> FeedbackView {
        let feedback = &stats.feedback;
        let row = |name: &str, count: &Comparison, percent: &Comparison| {
            [
                MedAvg::median(&format!("{name} Median"), count),
                MedAvg::mean(&format!("{name} Average"), count),
                MedAvg::median(&format!("{name} Median (by percent)"), percent),
                MedAvg::mean(&format!("{name} Average (by percent)"), percent),
            ]
        };

        FeedbackView {
            joins: figure(feedback.joins.value),
            unknown: figure(feedback.unknown.value),
            pie: Donut::of(&[
                ("ps-feedback-joins", feedback.joins.value as i64),
                ("ps-feedback-unknown", feedback.unknown.value as i64),
            ]),
            rows: vec![
                row("Join", &feedback.joins, &feedback.joins_percent),
                row("Unknown", &feedback.unknown, &feedback.unknown_percent),
            ],
        }
    }
}

/// A [`MedAvg`] as the page draws it.
#[derive(TemplateOnce)]
#[template(path = "pages/project_stats_med_avg.stpl.html")]
struct MedAvgInclude<'a> {
    graph: &'a MedAvg,
}

/// A [`Card`] as the page draws it.
#[derive(TemplateOnce)]
#[template(path = "pages/project_stats_card.stpl.html")]
struct CardInclude<'a> {
    card: &'a Card,
}

/// A [`DayChart`] as the page draws it.
#[derive(TemplateOnce)]
#[template(path = "pages/project_stats_day_chart.stpl.html")]
struct DayChartInclude<'a> {
    chart: &'a DayChart,
    /// What the chart shows, for those who cannot see it.
    label: &'a str,
}

/// A [`Donut`] as the page draws it, with a figure in its middle.
#[derive(TemplateOnce)]
#[template(path = "pages/project_stats_donut.stpl.html")]
struct DonutInclude<'a> {
    donut: &'a Donut,
    figure: &'a str,
    caption: &'a str,
}

/// Rows of [`BarRow`] as the page draws them.
#[derive(TemplateOnce)]
#[template(path = "pages/project_stats_bars.stpl.html")]
struct BarsInclude<'a> {
    rows: &'a [BarRow],
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    #[test]
    fn a_median_graph_places_the_project_at_its_percentile() {
        let comparison = Comparison {
            value: 12.0,
            median: 4.0,
            mean: 6.0,
            max: 24.0,
            percentile: 80.0,
        };

        let median = MedAvg::median("Median", &comparison);
        check!(median.midline == 50.0);
        check!(median.dot == 80.0);

        let mean = MedAvg::mean("Mean", &comparison);
        check!(mean.midline == 25.0);
        check!(mean.dot == 50.0);
    }

    #[test]
    fn a_donut_starts_at_the_top_and_goes_round() {
        let donut = Donut::of(&[("a", 1), ("b", 0), ("c", 3)]);

        check!(donut.segments.len() == 2);
        check!(donut.segments[0].share == 25.0);
        check!(donut.segments[0].offset == 25.0);
        check!(donut.segments[1].offset == 0.0);
    }

    #[test]
    fn a_day_chart_scales_to_its_largest_count() {
        let day = |day: &str, views, visits| DayCounts {
            day: day.to_string(),
            views,
            visits,
            ..DayCounts::default()
        };

        let chart = DayChart::conversions(&[day("2026-11-15", 10, 20), day("2026-11-16", 5, 0)]);

        check!(chart.top == "20");
        check!(chart.bars[0].height == 50.0);
        check!(chart.lines[0].points == "5.0,0.0 15.0,100.0");
        check!(chart.first == "Nov 15");
    }
}
