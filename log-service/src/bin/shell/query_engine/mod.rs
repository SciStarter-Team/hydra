use anyhow::Result;
use log_service::database::storage::Database;
use std::collections::HashMap;
use std::sync::Arc;

pub mod functions;

#[derive(Debug, Clone)]
pub enum QueryType {
    Function(String, HashMap<String, String>), // Function name and parameters
    RawSql(String),                            // Arbitrary SQL query
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub row_count: usize,
    pub execution_time_ms: u64,
}

pub struct QueryEngine {
    database: Arc<Database>,
}

impl QueryEngine {
    pub async fn new(database: Arc<Database>) -> Result<Self> {
        Ok(Self { database })
    }

    pub async fn execute_query(
        &self,
        query_type: QueryType,
        progress: functions::ProgressCallback,
    ) -> Result<QueryResult> {
        let start_time = std::time::Instant::now();

        let result = match query_type {
            QueryType::Function(function_name, parameters) => {
                self.execute_function(&function_name, parameters, progress)
                    .await?
            }
            QueryType::RawSql(sql) => self.execute_raw_sql(&sql).await?,
        };

        let execution_time_ms = start_time.elapsed().as_millis() as u64;
        let row_count = result.1.len();

        Ok(QueryResult {
            columns: result.0,
            rows: result.1,
            row_count,
            execution_time_ms,
        })
    }

    async fn execute_function(
        &self,
        function_name: &str,
        parameters: HashMap<String, String>,
        progress: functions::ProgressCallback,
    ) -> Result<(Vec<String>, Vec<Vec<String>>)> {
        // Find the registered function
        let func = functions::find_function(function_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown function: {}", function_name))?;

        // Validate parameters using the function's validator
        functions::validate_parameters(func, &parameters)?;

        // Execute the function
        func.execute(Arc::clone(&self.database), parameters, progress)
            .await
    }

    pub async fn execute_raw_sql(&self, sql: &str) -> Result<(Vec<String>, Vec<Vec<String>>)> {
        // Execute SQL query using DataFusion
        let ctx = self.database.context();
        let df = ctx.sql(sql).await?;
        let batches = df.collect().await?;

        if batches.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        // Extract column names from schema
        let schema = batches[0].schema();
        let columns: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();

        // Convert Arrow data directly to strings
        let mut rows = Vec::new();

        for batch in &batches {
            let num_rows = batch.num_rows();
            let num_cols = batch.num_columns();

            for row_idx in 0..num_rows {
                let mut row = Vec::new();

                for col_idx in 0..num_cols {
                    let array = batch.column(col_idx);
                    let value = self.extract_value_from_array(array.as_ref(), row_idx);
                    row.push(value);
                }

                rows.push(row);
            }
        }

        Ok((columns, rows))
    }

    fn extract_value_from_array(&self, array: &dyn datafusion::arrow::array::Array, row_idx: usize) -> String {
        use datafusion::arrow::array::*;

        if array.is_null(row_idx) {
            return "NULL".to_string();
        }

        match array.data_type() {
            datafusion::arrow::datatypes::DataType::Utf8 => {
                let string_array = array.as_any().downcast_ref::<StringArray>().unwrap();
                string_array.value(row_idx).to_string()
            }
            datafusion::arrow::datatypes::DataType::Int32 => {
                let int_array = array.as_any().downcast_ref::<Int32Array>().unwrap();
                int_array.value(row_idx).to_string()
            }
            datafusion::arrow::datatypes::DataType::Int64 => {
                let int_array = array.as_any().downcast_ref::<Int64Array>().unwrap();
                int_array.value(row_idx).to_string()
            }
            datafusion::arrow::datatypes::DataType::Float64 => {
                let float_array = array.as_any().downcast_ref::<Float64Array>().unwrap();
                float_array.value(row_idx).to_string()
            }
            datafusion::arrow::datatypes::DataType::Boolean => {
                let bool_array = array.as_any().downcast_ref::<BooleanArray>().unwrap();
                bool_array.value(row_idx).to_string()
            }
            datafusion::arrow::datatypes::DataType::Timestamp(_, _) => {
                let timestamp_array = array
                    .as_any()
                    .downcast_ref::<TimestampMicrosecondArray>()
                    .unwrap();
                // Convert microseconds to readable timestamp
                let micros = timestamp_array.value(row_idx);
                let secs = micros / 1_000_000;
                let nanos = (micros % 1_000_000) * 1000;
                match std::time::UNIX_EPOCH
                    .checked_add(std::time::Duration::new(secs as u64, nanos as u32))
                {
                    Some(datetime) => format!("{:?}", datetime),
                    None => micros.to_string(),
                }
            }
            _ => {
                // Fallback: use Arrow's display formatting
                format!("{:?}", array.slice(row_idx, 1))
            }
        }
    }

    pub fn get_available_functions(&self) -> Vec<String> {
        functions::get_registered_functions()
            .into_iter()
            .map(|f| f.name().to_string())
            .collect()
    }

    pub fn get_available_tables(&self) -> Option<Vec<String>> {
        let ctx = self.database.context();
        let catalog = ctx.catalog("datafusion")?;
        let schema = catalog.schema("public")?;
        
        let mut table_names = Vec::new();
        for name in schema.table_names() {
            table_names.push(name);
        }
        
        table_names.sort();
        Some(table_names)
    }

    pub fn get_context(&self) -> &datafusion::execution::context::SessionContext {
        self.database.context()
    }

    /// Parse function call syntax: function_name(param1="value1", param2="value2")
    /// Returns (function_name, parameters_map)
    pub fn parse_function_call(input: &str) -> Result<(String, HashMap<String, String>)> {
        let input = input.trim();

        // Check if it's a function call (contains parentheses)
        if let Some(paren_pos) = input.find('(') {
            if !input.ends_with(')') {
                return Err(anyhow::anyhow!(
                    "Invalid function call syntax: missing closing parenthesis"
                ));
            }

            let function_name = input[..paren_pos].trim().to_string();
            let params_str = &input[paren_pos + 1..input.len() - 1].trim();

            if params_str.is_empty() {
                return Ok((function_name, HashMap::new()));
            }

            let mut parameters = HashMap::new();

            // Simple parameter parsing: param1="value1", param2="value2"
            for param_pair in params_str.split(',') {
                let param_pair = param_pair.trim();
                if let Some(eq_pos) = param_pair.find('=') {
                    let key = param_pair[..eq_pos].trim().to_string();
                    let value_part = param_pair[eq_pos + 1..].trim();

                    // Remove quotes if present
                    let value = if (value_part.starts_with('"') && value_part.ends_with('"'))
                        || (value_part.starts_with('\'') && value_part.ends_with('\''))
                    {
                        value_part[1..value_part.len() - 1].to_string()
                    } else {
                        value_part.to_string()
                    };

                    parameters.insert(key, value);
                } else {
                    return Err(anyhow::anyhow!("Invalid parameter syntax: {}", param_pair));
                }
            }

            Ok((function_name, parameters))
        } else {
            // Simple function name without parameters
            Ok((input.to_string(), HashMap::new()))
        }
    }
}
