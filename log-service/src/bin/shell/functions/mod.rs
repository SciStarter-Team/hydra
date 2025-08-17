use anyhow::Result;

mod standard;

/// Helper function to convert analytics response to table format
pub fn convert_analytics_response_to_table(
    response: common::message::log::AnalyticsResponse,
    _title: &str,
) -> Result<(Vec<String>, Vec<Vec<String>>)> {
    use common::message::log::AnalyticsResponse;

    match response {
        AnalyticsResponse::WeeklyCounts(weekly_counts) => {
            let columns = vec!["Week Start".to_string(), "Count".to_string()];
            let rows: Vec<Vec<String>> = weekly_counts
                .into_iter()
                .map(|wc| vec![wc.week_start, wc.count.to_string()])
                .collect();
            Ok((columns, rows))
        }
        AnalyticsResponse::DailyCounts(daily_counts) => {
            let columns = vec!["Date".to_string(), "Count".to_string()];
            let rows: Vec<Vec<String>> = daily_counts
                .into_iter()
                .map(|dc| vec![dc.date, dc.count.to_string()])
                .collect();
            Ok((columns, rows))
        }
        AnalyticsResponse::MonthlyCounts(monthly_counts) => {
            let columns = vec!["Month Start".to_string(), "Count".to_string()];
            let rows: Vec<Vec<String>> = monthly_counts
                .into_iter()
                .map(|mc| vec![mc.month_start, mc.count.to_string()])
                .collect();
            Ok((columns, rows))
        }
    }
}
