// Analytics management for admin

use crate::component::{AdminLayout, Component, admin_layout::AdminLayoutPrepared};
use crate::middleware::authorization::Authenticated;
use common::message::log;
use common::message::user::{
    AdminAnalyticsRequest, AdminAnalyticsResponse, UserDetailsExportRequest,
    UserDetailsExportResponse,
};
use common::service::request_timeout;
use common::shared::db;
use jiff::Timestamp;
use poem::{
    Endpoint, IntoResponse, Route, get, handler,
    web::{Html, Query},
};
use sailfish::TemplateOnce;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use ulid::Ulid;

// Types
#[derive(Debug)]
pub struct AnalyticsPage {
    pub start_date: String,
    pub end_date: String,
    pub selected_days: Option<u32>,
    pub activity_page: u32,
    pub top_performers_criterion: TopPerformersCriterion,
}

#[derive(Debug, Clone)]
pub struct GeographicStat {
    pub country: String,
    pub country_code: String,
    pub user_count: i64,
}

#[derive(Debug, Clone)]
pub struct ActivityStat {
    pub date: String,
    pub new_users: i64,
    pub total_users_to_date: i64,
    pub verified_users_to_date: i64,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/analytics_recent_activity_table.stpl.html")]
pub struct AnalyticsRecentActivityTable {
    pub activity_stats: Vec<ActivityStat>,
    pub activity_page: u32,
    pub activity_total_pages: u32,
    pub activity_total_count: usize,
    pub selected_days: Option<u32>,
    pub start_date: String,
    pub end_date: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/analytics.stpl.html")]
pub struct AnalyticsPagePrepared {
    pub total_users: u32,
    pub verified_users: u32,
    pub weekly_signups: u32,
    pub daily_signups: u32,
    pub top_performers: Vec<TopPerformerItem>,
    pub geographic_stats: Vec<GeographicStat>,
    pub recent_signups_chart: Vec<ActivityStat>,
    pub activity_stats: Vec<ActivityStat>,
    pub activity_page: u32,
    pub activity_total_pages: u32,
    pub activity_total_count: usize,
    pub clickthrough_counts: Vec<WeeklyCount>,
    pub participation_counts: Vec<WeeklyCount>,
    pub selected_days: Option<u32>,
    pub start_date: String,
    pub end_date: String,
    pub top_performers_criterion: TopPerformersCriterion,
}

#[derive(Debug, Clone)]
pub struct WeeklyCount {
    pub week_start: String,
    pub count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TopPerformersCriterion {
    #[default]
    Views,
    Clickthroughs,
    Participation,
}

impl TopPerformersCriterion {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Views => "views",
            Self::Clickthroughs => "clickthroughs",
            Self::Participation => "participation",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Views => "viewed pages",
            Self::Clickthroughs => "clicked projects",
            Self::Participation => "participated projects",
        }
    }

    pub fn is_views(&self) -> bool {
        matches!(self, Self::Views)
    }

    pub fn is_clickthroughs(&self) -> bool {
        matches!(self, Self::Clickthroughs)
    }

    pub fn is_participation(&self) -> bool {
        matches!(self, Self::Participation)
    }
}

#[derive(Debug, Clone)]
pub struct TopPerformerItem {
    pub identifier: String,
    pub count: i64,
    pub display_name: Option<String>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/top_performers_box.stpl.html")]
pub struct TopPerformersBox {
    pub criterion: TopPerformersCriterion,
    pub items: Vec<TopPerformerItem>,
    pub days: Option<u32>,
    pub start_date: String,
    pub end_date: String,
}

// Query types
#[derive(Debug, Deserialize)]
struct AnalyticsPageQuery {
    days: Option<u32>,
    start_date: Option<String>,
    end_date: Option<String>,
    activity_page: Option<u32>,
    top_performers_criterion: Option<TopPerformersCriterion>,
}

#[derive(Debug, Deserialize)]
struct RecentActivityTableQuery {
    days: Option<u32>,
    start_date: Option<String>,
    end_date: Option<String>,
    activity_page: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct TopPerformersQuery {
    days: Option<u32>,
    start_date: Option<String>,
    end_date: Option<String>,
    criterion: Option<TopPerformersCriterion>,
}

#[derive(Debug, Deserialize)]
struct ExportQuery {
    days: Option<u32>,
    start_date: Option<String>,
    end_date: Option<String>,
}

// Helper functions
fn determine_date_range(
    days: Option<u32>,
    start_date: Option<String>,
    end_date: Option<String>,
) -> (String, String, Option<u32>) {
    use jiff::ToSpan;

    if let (Some(start), Some(end)) = (start_date, end_date) {
        // Use custom date range
        (start, end, None)
    } else {
        // Calculate date range from days parameter
        let days_count = days.unwrap_or(30);
        let end = Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date();
        let start = end
            .checked_sub(((days_count - 1) as i64).days())
            .unwrap_or(end);
        (start.to_string(), end.to_string(), Some(days_count))
    }
}

fn escape_csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

// Component implementation
impl Component for AnalyticsPage {
    type Param = Option<Arc<common::message::user::BasicInfo>>;
    type Prepared = AdminLayoutPrepared<AnalyticsPagePrepared>;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        // Fetch user analytics from user service
        let user_analytics = request_timeout(
            "user.stats.admin_analytics",
            AdminAnalyticsRequest {},
            Duration::from_secs(5),
        )
        .await
        .unwrap_or(AdminAnalyticsResponse {
            total_users: 0,
            verified_users: 0,
            monthly_signups: 0,
            weekly_signups: 0,
            daily_signups: 0,
        });

        // Get top performers using the selected criterion
        let top_performers_response = request_timeout(
            "log.top_performers_analytics",
            log::TopPerformersQuery {
                start_date: self.start_date.clone(),
                end_date: self.end_date.clone(),
                criterion: self.top_performers_criterion.as_str().to_string(),
                limit: 5,
            },
            Duration::from_secs(5),
        )
        .await;

        let top_performers = match top_performers_response {
            Ok(log::AnalyticsResponse::TopPerformers(items)) => {
                let mut performers = Vec::new();

                for item in items {
                    let display_name = if self.top_performers_criterion
                        == TopPerformersCriterion::Clickthroughs
                        || self.top_performers_criterion == TopPerformersCriterion::Participation
                    {
                        if let Ok(_project_id) = item.identifier.parse::<Ulid>() {
                            sqlx::query!(
                                r#"SELECT name FROM projects WHERE project_id = ?"#,
                                item.identifier
                            )
                            .fetch_optional(db())
                            .await
                            .ok()
                            .flatten()
                            .map(|row| row.name)
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    performers.push(TopPerformerItem {
                        identifier: item.identifier,
                        count: item.count,
                        display_name,
                    });
                }

                performers
            }
            Err(e) => {
                eprintln!("Top performers analytics error: {:?}", e);
                Vec::new()
            }
            _ => {
                eprintln!("Top performers analytics returned unexpected variant");
                Vec::new()
            }
        };

        // Fetch geographic analytics
        let geographic_response = request_timeout(
            "user.stats.geographic",
            common::message::user::GeographicAnalyticsRequest {
                start_date: self.start_date.clone(),
                end_date: self.end_date.clone(),
            },
            Duration::from_secs(5),
        )
        .await
        .unwrap_or(common::message::user::GeographicAnalyticsResponse { stats: vec![] });

        let geographic_stats: Vec<GeographicStat> = geographic_response
            .stats
            .into_iter()
            .map(|s| GeographicStat {
                country: s.country,
                country_code: s.country_code,
                user_count: s.user_count,
            })
            .collect();

        // Fetch last 14 days for the chart
        let chart_result: Result<common::message::user::ActivityAnalyticsResponse, _> =
            request_timeout(
                "user.stats.activity",
                common::message::user::ActivityAnalyticsRequest {
                    start_date: self.start_date.clone(),
                    end_date: self.end_date.clone(),
                    page: 1,
                    per_page: 14,
                },
                Duration::from_secs(5),
            )
            .await;

        let recent_signups_chart: Vec<ActivityStat> = match chart_result {
            Ok(response) => response
                .stats
                .into_iter()
                .map(|s| ActivityStat {
                    date: s.date,
                    new_users: s.new_users,
                    total_users_to_date: s.total_users_to_date,
                    verified_users_to_date: s.verified_users_to_date,
                })
                .collect(),
            Err(e) => {
                eprintln!("Chart data error: {:?}", e);
                Vec::new()
            }
        };

        // Fetch activity analytics with pagination
        let per_page = 30;
        let activity_result: Result<common::message::user::ActivityAnalyticsResponse, _> =
            request_timeout(
                "user.stats.activity",
                common::message::user::ActivityAnalyticsRequest {
                    start_date: self.start_date.clone(),
                    end_date: self.end_date.clone(),
                    page: self.activity_page,
                    per_page,
                },
                Duration::from_secs(5),
            )
            .await;

        let activity_response = match activity_result {
            Ok(response) => response,
            Err(e) => {
                eprintln!("Activity analytics error: {:?}", e);
                common::message::user::ActivityAnalyticsResponse {
                    stats: vec![],
                    total_count: 0,
                }
            }
        };

        let activity_stats: Vec<ActivityStat> = activity_response
            .stats
            .into_iter()
            .map(|s| ActivityStat {
                date: s.date,
                new_users: s.new_users,
                total_users_to_date: s.total_users_to_date,
                verified_users_to_date: s.verified_users_to_date,
            })
            .collect();

        let activity_total_count = activity_response.total_count;
        let activity_total_pages =
            ((activity_total_count + per_page as usize - 1) / per_page as usize) as u32;
        let activity_page = self.activity_page.min(activity_total_pages.max(1));

        // Fetch clickthrough analytics
        let clickthrough_analytics = request_timeout(
            "log.clickthrough_analytics",
            log::AnalyticsQuery {
                start_date: self.start_date.clone(),
                end_date: self.end_date.clone(),
            },
            Duration::from_secs(5),
        )
        .await;

        let clickthrough_counts = match clickthrough_analytics {
            Ok(log::AnalyticsResponse::WeeklyCounts(counts)) => counts
                .into_iter()
                .map(|c| WeeklyCount {
                    week_start: c.week_start,
                    count: c.count,
                })
                .collect(),
            Err(e) => {
                eprintln!("Clickthrough analytics error: {:?}", e);
                Vec::new()
            }
            _ => {
                eprintln!("Clickthrough analytics returned unexpected variant");
                Vec::new()
            }
        };

        // Fetch participation analytics
        let participation_analytics = request_timeout(
            "log.participation_analytics",
            log::AnalyticsQuery {
                start_date: self.start_date.clone(),
                end_date: self.end_date.clone(),
            },
            Duration::from_secs(5),
        )
        .await;

        let participation_counts = match participation_analytics {
            Ok(log::AnalyticsResponse::WeeklyCounts(counts)) => counts
                .into_iter()
                .map(|c| WeeklyCount {
                    week_start: c.week_start,
                    count: c.count,
                })
                .collect(),
            Err(e) => {
                eprintln!("Participation analytics error: {:?}", e);
                Vec::new()
            }
            _ => {
                eprintln!("Participation analytics returned unexpected variant");
                Vec::new()
            }
        };

        let analytics_content = AnalyticsPagePrepared {
            total_users: user_analytics.total_users,
            verified_users: user_analytics.verified_users,
            weekly_signups: user_analytics.weekly_signups,
            daily_signups: user_analytics.daily_signups,
            top_performers,
            geographic_stats,
            recent_signups_chart,
            activity_stats,
            activity_page,
            activity_total_pages,
            activity_total_count,
            clickthrough_counts,
            participation_counts,
            selected_days: self.selected_days,
            start_date: self.start_date.clone(),
            end_date: self.end_date.clone(),
            top_performers_criterion: self.top_performers_criterion.clone(),
        };

        AdminLayout::new("Analytics", analytics_content)
            .with_breadcrumb("Analytics", None)
            .with_sidebar_active("analytics")
            .prepare(param)
            .await
    }
}

// Handlers
#[handler]
async fn analytics(
    Query(query): Query<AnalyticsPageQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let (start_date, end_date, selected_days) =
        determine_date_range(query.days, query.start_date, query.end_date);

    AnalyticsPage {
        start_date,
        end_date,
        selected_days,
        activity_page: query.activity_page.unwrap_or(1),
        top_performers_criterion: query.top_performers_criterion.unwrap_or_default(),
    }
    .into_response(user_info)
    .await
}

#[handler]
async fn analytics_recent_activity_table(
    Query(query): Query<RecentActivityTableQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, anyhow::Error> {
    let (start_date, end_date, selected_days) =
        determine_date_range(query.days, query.start_date, query.end_date);

    let activity_page = query.activity_page.unwrap_or(1);

    // Fetch paginated activity data
    let activity_result: Result<common::message::user::ActivityAnalyticsResponse, _> =
        request_timeout(
            "user.stats.activity",
            common::message::user::ActivityAnalyticsRequest {
                start_date: start_date.clone(),
                end_date: end_date.clone(),
                page: activity_page,
                per_page: 30,
            },
            Duration::from_secs(5),
        )
        .await;

    let (activity_stats, activity_total_count): (Vec<ActivityStat>, usize) = match activity_result {
        Ok(response) => {
            let stats = response
                .stats
                .into_iter()
                .map(|s| ActivityStat {
                    date: s.date,
                    new_users: s.new_users,
                    total_users_to_date: s.total_users_to_date,
                    verified_users_to_date: s.verified_users_to_date,
                })
                .collect();
            (stats, response.total_count)
        }
        Err(e) => {
            eprintln!("Activity stats error: {:?}", e);
            (Vec::new(), 0)
        }
    };

    let activity_total_pages = (activity_total_count as f64 / 30.0).ceil() as u32;

    Ok(Html(
        AnalyticsRecentActivityTable {
            activity_stats,
            activity_page,
            activity_total_pages,
            activity_total_count,
            selected_days,
            start_date,
            end_date,
        }
        .render_once()?,
    ))
}

#[handler]
async fn analytics_top_performers(
    Query(query): Query<TopPerformersQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, anyhow::Error> {
    let (start_date, end_date, days) =
        determine_date_range(query.days, query.start_date, query.end_date);

    let criterion = query.criterion.unwrap_or_default();

    // Fetch top performers
    let top_performers_response = request_timeout(
        "log.top_performers_analytics",
        log::TopPerformersQuery {
            start_date: start_date.clone(),
            end_date: end_date.clone(),
            criterion: criterion.as_str().to_string(),
            limit: 5,
        },
        Duration::from_secs(5),
    )
    .await;

    let items = match top_performers_response {
        Ok(log::AnalyticsResponse::TopPerformers(items)) => {
            let mut performers = Vec::new();

            for item in items {
                let display_name = if criterion == TopPerformersCriterion::Clickthroughs
                    || criterion == TopPerformersCriterion::Participation
                {
                    if let Ok(_project_id) = item.identifier.parse::<Ulid>() {
                        sqlx::query!(
                            r#"SELECT name FROM projects WHERE project_id = ?"#,
                            item.identifier
                        )
                        .fetch_optional(db())
                        .await
                        .ok()
                        .flatten()
                        .map(|row| row.name)
                    } else {
                        None
                    }
                } else {
                    None
                };

                performers.push(TopPerformerItem {
                    identifier: item.identifier,
                    count: item.count,
                    display_name,
                });
            }

            performers
        }
        Err(e) => {
            eprintln!("Top performers analytics error: {:?}", e);
            Vec::new()
        }
        _ => {
            eprintln!("Top performers analytics returned unexpected variant");
            Vec::new()
        }
    };

    Ok(Html(
        TopPerformersBox {
            criterion,
            items,
            days,
            start_date,
            end_date,
        }
        .render_once()?,
    ))
}

#[handler]
async fn analytics_export_users(
    Query(query): Query<ExportQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, anyhow::Error> {
    use jiff::ToSpan;

    let (start_date, end_date) =
        if let (Some(start), Some(end)) = (query.start_date, query.end_date) {
            (start, end)
        } else {
            let days = query.days.unwrap_or(30);
            let end = Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date();
            let start = end.checked_sub(((days - 1) as i64).days()).unwrap_or(end);
            (start.to_string(), end.to_string())
        };

    let mut csv = String::new();
    csv.push_str("User ID,Handle,First Name,Last Name,Primary Email,Active,Staff,Superuser,Page Manager,Verified,Date Joined\n");

    let page_size = 500;
    let mut page = 1;

    loop {
        let export_response: UserDetailsExportResponse = request_timeout(
            "user.admin.user_export",
            UserDetailsExportRequest {
                start_date: start_date.clone(),
                end_date: end_date.clone(),
                page: Some(page),
                limit: Some(page_size),
            },
            Duration::from_secs(60),
        )
        .await?;

        let count = export_response.users.len();

        for user in export_response.users {
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{},{}\n",
                escape_csv_field(&user.user_id.to_string()),
                escape_csv_field(&user.handle),
                escape_csv_field(&user.first_name),
                escape_csv_field(&user.last_name),
                escape_csv_field(&user.primary_email),
                user.is_active,
                user.is_staff,
                user.is_superuser,
                user.is_page_manager,
                user.verified,
                escape_csv_field(&user.date_joined),
            ));
        }

        if count < page_size as usize {
            break;
        }

        page += 1;
    }

    Ok(poem::Response::builder()
        .content_type("text/csv")
        .header("Content-Disposition", "attachment; filename=\"users.csv\"")
        .body(csv))
}

#[handler]
async fn analytics_export_projects(
    Query(query): Query<ExportQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, anyhow::Error> {
    use jiff::ToSpan;

    let (start_date, end_date) =
        if let (Some(start), Some(end)) = (query.start_date, query.end_date) {
            (start, end)
        } else {
            let days = query.days.unwrap_or(30);
            let end = Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date();
            let start = end.checked_sub(((days - 1) as i64).days()).unwrap_or(end);
            (start.to_string(), end.to_string())
        };

    struct ProjectRow {
        project_id: String,
        name: String,
        status: String,
        created: String,
        origin: String,
        presenter: String,
        url: String,
        publish: Option<bool>,
    }

    let project_rows = sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT
            p.project_id,
            p.name,
            p.status,
            p.created,
            p.origin,
            p.presenter,
            p.url,
            e.publish
        FROM projects p
        LEFT JOIN entities e ON e.target = p.project_id AND e.kind = 'Project'
        WHERE p.created >= ? AND p.created <= ?
        ORDER BY p.created DESC
        "#,
        start_date,
        end_date
    )
    .fetch_all(db())
    .await?;

    let mut csv = String::new();
    csv.push_str("Project ID,Name,Status,Created,Origin,Presenter,URL,Published\n");

    for project in project_rows {
        let published = project.publish.unwrap_or(false);
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            escape_csv_field(&project.project_id),
            escape_csv_field(&project.name),
            escape_csv_field(&project.status),
            escape_csv_field(&project.created),
            escape_csv_field(&project.origin),
            escape_csv_field(&project.presenter),
            escape_csv_field(&project.url),
            published,
        ));
    }

    Ok(poem::Response::builder()
        .content_type("text/csv")
        .header(
            "Content-Disposition",
            "attachment; filename=\"projects.csv\"",
        )
        .body(csv))
}

#[handler]
async fn analytics_export_events(
    Query(query): Query<ExportQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<impl IntoResponse, anyhow::Error> {
    use jiff::ToSpan;

    let (start_date, end_date) =
        if let (Some(start), Some(end)) = (query.start_date, query.end_date) {
            (start, end)
        } else {
            let days = query.days.unwrap_or(30);
            let end = Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date();
            let start = end.checked_sub(((days - 1) as i64).days()).unwrap_or(end);
            (start.to_string(), end.to_string())
        };

    struct EventRow {
        event_id: String,
        name: String,
        created: String,
        origin: String,
        presenter: String,
        url: String,
        event_type: String,
        location_type: String,
        publish: Option<bool>,
    }

    let event_rows = sqlx::query_as!(
        EventRow,
        r#"
        SELECT
            ev.event_id,
            ev.name,
            ev.created,
            ev.origin,
            ev.presenter,
            ev.url,
            ev.event_type,
            ev.location_type,
            e.publish
        FROM events ev
        LEFT JOIN entities e ON e.target = ev.event_id AND e.kind = 'Event'
        WHERE ev.created >= ? AND ev.created <= ?
        ORDER BY ev.created DESC
        "#,
        start_date,
        end_date
    )
    .fetch_all(db())
    .await?;

    let mut csv = String::new();
    csv.push_str("Event ID,Name,Created,Origin,Presenter,URL,Event Type,Location Type,Published\n");

    for event in event_rows {
        let published = event.publish.unwrap_or(false);
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            escape_csv_field(&event.event_id),
            escape_csv_field(&event.name),
            escape_csv_field(&event.created),
            escape_csv_field(&event.origin),
            escape_csv_field(&event.presenter),
            escape_csv_field(&event.url),
            escape_csv_field(&event.event_type),
            escape_csv_field(&event.location_type),
            published,
        ));
    }

    Ok(poem::Response::builder()
        .content_type("text/csv")
        .header("Content-Disposition", "attachment; filename=\"events.csv\"")
        .body(csv))
}

// Routes export
pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(analytics))
        .at(
            "/recent-activity-table",
            get(analytics_recent_activity_table),
        )
        .at("/top-performers", get(analytics_top_performers))
        .at("/export/users", get(analytics_export_users))
        .at("/export/projects", get(analytics_export_projects))
        .at("/export/events", get(analytics_export_events))
}
