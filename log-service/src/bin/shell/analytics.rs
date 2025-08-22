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

        // Apply intelligent width constraints based on terminal width
        self.apply_width_constraints(&mut col_widths, terminal_width);

        // Create header row using disambiguated columns with truncation
        let header = disambiguated_columns
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let width = col_widths.get(i).copied().unwrap_or(20);
                let truncated = if h.len() > width {
                    format!("{}...", &h[..width.saturating_sub(3)])
                } else {
                    h.clone()
                };
                format!("{:width$}", truncated, width = width)
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

        // Add SQL keyword suggestions
        for keyword in sql_keywords {
            if keyword.to_lowercase().starts_with(&input_lower) {
                suggestions.push(keyword.to_string());
            }
        }

        // Get table suggestions dynamically from DataFusion session context
        if let Some(tables) = self.query_engine.get_available_tables() {
            for table in tables {
                if table.starts_with(&input_lower) {
                    suggestions.push(table);
                }
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

    fn apply_width_constraints(&self, col_widths: &mut [usize], terminal_width: Option<usize>) {
        if col_widths.is_empty() {
            return;
        }

        // Default fallback if no terminal width provided
        let available_width = terminal_width.unwrap_or(80).saturating_sub(4); // Account for borders

        // Calculate total width needed with separators
        let separator_width = (col_widths.len().saturating_sub(1)) * 3; // " | " between columns
        let total_content_width: usize = col_widths.iter().sum();
        let total_table_width = total_content_width + separator_width;

        if total_table_width <= available_width {
            // Table fits comfortably, just apply reasonable max limits
            for width in col_widths {
                *width = (*width).min(50);
            }
            return;
        }

        // Table is too wide - need to intelligently distribute space
        let target_content_width = available_width.saturating_sub(separator_width);

        // Strategy: Proportional reduction with minimum widths
        let min_col_width = 8; // Minimum readable width
        let num_cols = col_widths.len();

        // Ensure we can fit at least minimum widths
        if target_content_width < num_cols * min_col_width {
            // Terminal too narrow, fall back to minimum widths
            for width in col_widths {
                *width = min_col_width;
            }
            return;
        }

        // Proportional reduction while respecting minimums
        let reduction_factor = target_content_width as f64 / total_content_width as f64;
        let mut remaining_width = target_content_width;

        // First pass: apply proportional reduction with minimum constraint
        for width in col_widths.iter_mut() {
            let proposed_width = (*width as f64 * reduction_factor).round() as usize;
            *width = proposed_width.max(min_col_width);
            remaining_width = remaining_width.saturating_sub(*width);
        }

        // Second pass: distribute any remaining width to columns that aren't at minimum
        while remaining_width > 0 {
            let mut distributed = false;
            for width in col_widths.iter_mut() {
                if *width > min_col_width && remaining_width > 0 {
                    *width += 1;
                    remaining_width -= 1;
                    distributed = true;
                }
            }
            if !distributed {
                break; // All columns at minimum, can't distribute more
            }
        }
    }
}
