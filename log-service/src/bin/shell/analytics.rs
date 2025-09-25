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
        terminal_width: Option<usize>,
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

        // Check if we should use row-per-column mode
        let available_width = terminal_width.unwrap_or(80).saturating_sub(4); // Account for borders
        let separator_width = (col_widths.len().saturating_sub(1)) * 3; // " | " between columns
        let total_content_width: usize = col_widths.iter().sum();
        let total_table_width = total_content_width + separator_width;

        if total_table_width > available_width && col_widths.len() > 1 {
            // Switch to row-per-column mode - no truncation needed
            output.extend(self.format_as_row_per_column(&disambiguated_columns, display_rows));
        } else {
            // Use traditional column mode without width constraints (no truncation)
            output.extend(self.format_as_columns(
                &disambiguated_columns,
                display_rows,
                &col_widths,
            ));
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

        // SQL keyword suggestions (excluding shell commands and statement starters)
        let sql_keywords = vec![
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
        ];

        // Add SQL keyword suggestions
        for keyword in sql_keywords {
            if keyword.to_lowercase().starts_with(&input_lower) {
                suggestions.push(keyword.to_string());
            }
        }

        // Get table suggestions dynamically from DataFusion session context
        if let Some(tables) = self.query_engine.get_available_tables() {
            for table in tables {
                if table.to_lowercase().starts_with(&input_lower) {
                    suggestions.push(table);
                }
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

    fn format_as_row_per_column(&self, columns: &[String], rows: &[Vec<String>]) -> Vec<String> {
        let mut output = Vec::new();

        // Find the maximum column name width for consistent alignment
        let max_col_name_width = columns.iter().map(|name| name.len()).max().unwrap_or(10);

        for (row_idx, row) in rows.iter().enumerate() {
            if row_idx > 0 {
                output.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());
            }

            for (col_idx, column_name) in columns.iter().enumerate() {
                let cell_value = row.get(col_idx).map(|s| s.as_str()).unwrap_or("");
                // No truncation - show full values
                output.push(format!(
                    "{:>width$}: {}",
                    column_name,
                    cell_value,
                    width = max_col_name_width
                ));
            }
        }

        output
    }

    fn format_as_columns(
        &self,
        columns: &[String],
        rows: &[Vec<String>],
        col_widths: &[usize],
    ) -> Vec<String> {
        let mut output = Vec::new();

        // Create header row without truncation
        let header = columns
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let width = col_widths.get(i).copied().unwrap_or(h.len());
                format!("{:width$}", h, width = width)
            })
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

        // Add data rows without truncation
        for row in rows {
            let formatted_row = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    let width = col_widths.get(i).copied().unwrap_or(cell.len());
                    format!("{:width$}", cell, width = width)
                })
                .collect::<Vec<_>>()
                .join(" | ");
            output.push(formatted_row);
        }

        output
    }
}
