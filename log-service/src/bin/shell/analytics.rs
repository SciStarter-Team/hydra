use anyhow::Result;
use csv::Writer;
use log_service::database::storage::Database;
use std::sync::Arc;

use crate::query_engine::{QueryEngine, QueryResult, QueryType};

pub struct AnalyticsEngine {
    query_engine: QueryEngine,
}

impl AnalyticsEngine {
    pub async fn new(database: Arc<Database>) -> Result<Self> {
        let query_engine = QueryEngine::new(database.clone()).await?;
        Ok(Self { query_engine })
    }

    pub async fn execute_query(
        &self,
        query_type: QueryType,
        progress: crate::query_engine::functions::ProgressCallback,
    ) -> Result<QueryResult> {
        self.query_engine.execute_query(query_type, progress).await
    }

    pub fn export_to_csv(&self, result: &QueryResult, filename: &str) -> Result<()> {
        let mut writer = Writer::from_path(filename)?;

        // Write headers
        writer.write_record(&result.columns)?;

        // Write data rows
        for row in &result.rows {
            writer.write_record(row)?;
        }

        writer.flush()?;
        Ok(())
    }

    pub fn format_execution_time(ms: u64) -> String {
        if ms < 1000 {
            format!("{}ms", ms)
        } else if ms < 60_000 {
            let seconds = ms as f64 / 1000.0;
            format!("{:.1}s", seconds)
        } else {
            let minutes = ms / 60_000;
            let seconds = (ms % 60_000) as f64 / 1000.0;
            format!("{}m {:.1}s", minutes, seconds)
        }
    }

    pub fn format_result_for_display(
        &self,
        result: &QueryResult,
        max_rows: Option<usize>,
    ) -> Vec<String> {
        let mut output = Vec::new();

        // Add header with metadata
        output.push(format!(
            "Query executed in {} | {} rows returned",
            Self::format_execution_time(result.execution_time_ms),
            result.row_count
        ));
        output.push(String::new());

        if result.columns.is_empty() {
            output.push("No data returned".to_string());
            return output;
        }

        // Disambiguate duplicate column names
        let disambiguated_columns = self.disambiguate_column_names(&result.columns);

        // Calculate column widths using disambiguated names
        let mut col_widths: Vec<usize> = disambiguated_columns.iter().map(|h| h.len()).collect();

        let display_rows = if let Some(max) = max_rows {
            &result.rows[..result.rows.len().min(max)]
        } else {
            &result.rows
        };

        for row in display_rows {
            for (i, cell) in row.iter().enumerate() {
                if i < col_widths.len() {
                    col_widths[i] = col_widths[i].max(cell.len());
                }
            }
        }

        // Cap column widths for display
        for width in &mut col_widths {
            *width = (*width).min(50);
        }

        // Create header row using disambiguated columns
        let header = disambiguated_columns
            .iter()
            .enumerate()
            .map(|(i, h)| format!("{:width$}", h, width = col_widths[i]))
            .collect::<Vec<_>>()
            .join(" | ");
        output.push(header);

        // Create separator
        let separator = col_widths
            .iter()
            .map(|&w| "-".repeat(w))
            .collect::<Vec<_>>()
            .join("-|-");
        output.push(separator);

        // Add data rows
        for row in display_rows {
            let formatted_row = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    let width = col_widths.get(i).copied().unwrap_or(20);
                    let truncated = if cell.len() > width {
                        format!("{}...", &cell[..width.saturating_sub(3)])
                    } else {
                        cell.clone()
                    };
                    format!("{:width$}", truncated, width = width)
                })
                .collect::<Vec<_>>()
                .join(" | ");
            output.push(formatted_row);
        }

        // Add truncation notice if needed
        if let Some(max) = max_rows {
            if result.rows.len() > max {
                output.push(String::new());
                output.push(format!(
                    "... showing {} of {} rows (export to CSV to see all)",
                    max,
                    result.rows.len()
                ));
            }
        }

        output
    }

    pub fn get_query_suggestions(&self, input: &str) -> Vec<String> {
        let mut suggestions = Vec::new();

        let input_lower = input.to_lowercase();

        // SQL keyword suggestions
        let sql_keywords = vec![
            "SELECT",
            "FROM",
            "WHERE",
            "GROUP BY",
            "ORDER BY",
            "LIMIT",
            "COUNT",
            "SUM",
            "AVG",
            "MAX",
            "MIN",
            "DISTINCT",
            "JOIN",
            "INNER JOIN",
            "LEFT JOIN",
            "RIGHT JOIN",
            "ON",
            "AND",
            "OR",
            "NOT",
            "LIKE",
            "IN",
            "DESCRIBE",
            "quit",
            "exit",
            "help",
            "list",
            "export",
        ];

        // Table suggestions
        let tables = vec![
            "activity_log",
            "page_views",
            "clickthroughs",
            "participation",
            "bookmark",
            "join_table",
            "clickthrough_daily_summary",
            "clickthrough_weekly_summary",
            "join_daily_summary",
            "join_weekly_summary",
            "user_tags_cache",
            "user_emails_cache",
            "user_tags_emails_cache",
        ];

        // Add SQL keyword suggestions
        for keyword in sql_keywords {
            if keyword.to_lowercase().starts_with(&input_lower) {
                suggestions.push(keyword.to_string());
            }
        }

        // Add table suggestions
        for table in tables {
            if table.starts_with(&input_lower) {
                suggestions.push(table.to_string());
            }
        }

        // Add function suggestions
        for func in self.query_engine.get_available_functions() {
            if func.starts_with(&input_lower) {
                suggestions.push(func);
            }
        }

        suggestions.sort();
        suggestions.dedup();
        suggestions
    }

    fn disambiguate_column_names(&self, columns: &[String]) -> Vec<String> {
        let mut disambiguated = Vec::new();
        let mut name_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut name_indices: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        // Count occurrences of each column name
        for col in columns {
            *name_counts.entry(col.clone()).or_insert(0) += 1;
        }

        // Generate disambiguated names
        for col in columns {
            let count = name_counts[col];
            if count == 1 {
                // No duplicates, use original name
                disambiguated.push(col.clone());
            } else {
                // Duplicates exist, add suffix
                let index = name_indices.entry(col.clone()).or_insert(0);
                *index += 1;
                disambiguated.push(format!("{}#{}", col, index));
            }
        }

        disambiguated
    }
}
