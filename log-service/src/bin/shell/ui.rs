use anyhow::Result;
use ratatui::{prelude::*, widgets::*};
use std::sync::Arc;

use crate::analytics::AnalyticsEngine;
use crate::query_engine::{QueryEngine, QueryResult, QueryType};

pub struct App {
    query_engine: Arc<QueryEngine>,
    analytics_engine: Arc<AnalyticsEngine>,
    _debug: bool,

    // Command execution state
    cancellation_token: Option<tokio_util::sync::CancellationToken>,
    progress_receiver: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
    result_receiver: Option<tokio::sync::oneshot::Receiver<Result<QueryResult>>>,

    // Command input (always focused)
    command_input: String,
    command_cursor: usize,

    // Display area content
    display_content: Vec<String>,
    display_scroll: usize,
    manual_scroll_mode: bool, // true if user manually scrolled away from bottom

    // Tab completion
    completions: Vec<String>,
    completion_prefix: String,

    // Current query result for export
    current_result: Option<QueryResult>,

    // Command history
    command_history: Vec<String>,
    history_index: Option<usize>, // None means not browsing history
    temp_input: String,           // Stores current input when browsing history

    // Status
    status_message: String,

    // Track actual visible height for accurate scroll calculations
    last_visible_height: usize,

    // Track current terminal width for wrapping
    current_terminal_width: usize,
}

impl App {
    pub fn new(query_engine: QueryEngine, analytics_engine: AnalyticsEngine, debug: bool) -> Self {
        let app = Self {
            query_engine: Arc::new(query_engine),
            analytics_engine: Arc::new(analytics_engine),
            _debug: debug,
            cancellation_token: None,
            progress_receiver: None,
            result_receiver: None,
            command_input: String::new(),
            command_cursor: 0,
            display_content: Vec::new(),
            display_scroll: 0,
            manual_scroll_mode: false,
            completions: Vec::new(),
            completion_prefix: String::new(),
            current_result: None,
            command_history: Vec::new(),
            history_index: None,
            temp_input: String::new(),
            status_message: "Welcome to Analytics Shell! Type a command and press Enter."
                .to_string(),
            last_visible_height: 20,    // Default value
            current_terminal_width: 80, // Default value
        };

        // Don't show welcome until we know the terminal width
        app
    }

    fn show_welcome(&mut self, terminal_width: usize) {
        // Get all registered functions dynamically
        let function_names: Vec<String> = self.query_engine.get_available_functions();

        let mut content = vec![
            "Analytics Shell - Interactive Query Interface".to_string(),
            "".to_string(),
            "Available Commands:".to_string(),
        ];

        // Add functions with manual line wrapping using actual terminal width
        if function_names.is_empty() {
            content.push("• Functions: None available".to_string());
        } else {
            let prefix = "• Functions: ";
            let continuation_prefix = "             "; // Same length as "• Functions: "
            // Use available terminal width minus padding for borders/margins
            let max_line_length = terminal_width.saturating_sub(4); // Account for borders
            let available_width = max_line_length.saturating_sub(prefix.len());

            let mut current_line = String::new();
            let mut first_line = true;

            for (i, function_name) in function_names.iter().enumerate() {
                let separator = if i == 0 { "" } else { ", " };
                let addition = format!("{}{}", separator, function_name);

                if current_line.len() + addition.len() <= available_width {
                    current_line.push_str(&addition);
                } else {
                    // Add the current line as a separate content entry
                    let line_prefix = if first_line {
                        prefix
                    } else {
                        continuation_prefix
                    };
                    content.push(format!("{}{}", line_prefix, current_line));
                    current_line = function_name.clone();
                    first_line = false;
                }
            }

            // Add the final line
            if !current_line.is_empty() {
                let line_prefix = if first_line {
                    prefix
                } else {
                    continuation_prefix
                };
                content.push(format!("{}{}", line_prefix, current_line));
            }
        }

        content.extend(vec![
            "• SQL: SELECT * FROM activity_log LIMIT 10".to_string(),
            "• Special: help, list, export filename.csv, describe table_name".to_string(),
            "".to_string(),
            "Tab completion is available for functions and SQL keywords.".to_string(),
            "Use ↑/↓ arrow keys to navigate command history.".to_string(),
            "Type 'help' for detailed information.".to_string(),
            "".to_string(),
        ]);

        self.display_content = content;
    }

    fn show_list(&mut self, terminal_width: usize) {
        use crate::query_engine::functions;

        self.display_content.extend(vec![
            "".to_string(),
            "Available Functions:".to_string(),
            "==================".to_string(),
        ]);

        // List all registered functions with descriptions
        let registered_functions = functions::get_registered_functions();
        if registered_functions.is_empty() {
            self.display_content
                .push("No functions available".to_string());
        } else {
            for (i, func) in registered_functions.iter().enumerate() {
                // Add spacing between functions (but not before the first one)
                if i > 0 {
                    self.display_content.push("".to_string());
                }

                // Function name and description with wrapping
                let max_width = terminal_width.saturating_sub(4); // Account for borders
                let name_desc = format!("• {} - {}", func.name(), func.description());
                if name_desc.len() <= max_width {
                    self.display_content.push(name_desc);
                } else {
                    // Wrap long descriptions
                    self.display_content.push(format!("• {}", func.name()));
                    self.display_content
                        .push(format!("  {}", func.description()));
                }

                // Usage example with wrapping
                let usage = format!("  Usage: {}", func.usage_example());
                if usage.len() <= max_width {
                    self.display_content.push(usage);
                } else {
                    // Wrap long usage examples
                    self.display_content.push("  Usage:".to_string());
                    self.display_content
                        .push(format!("    {}", func.usage_example()));
                }
            }
        }

        self.display_content.extend(vec![
            "".to_string(),
            "Available Tables:".to_string(),
            "================".to_string(),
        ]);

        // Get tables dynamically from DataFusion context
        let ctx = self.query_engine.get_context();
        let mut table_names: Vec<String> = Vec::new();

        // Get table names from the default schema
        if let Some(catalog) = ctx.catalog("datafusion") {
            if let Some(schema) = catalog.schema("public") {
                table_names = schema.table_names();
                table_names.sort();
            }
        }

        if table_names.is_empty() {
            self.display_content.push("No tables available".to_string());
        } else {
            for table_name in table_names {
                self.display_content.push(format!("• {}", table_name));
            }
        }

        self.display_content.extend(vec![
            "".to_string(),
            "Use 'describe <table_name>' to see table structure.".to_string(),
            "".to_string(),
        ]);
    }

    pub fn render(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),    // Display area
                Constraint::Length(3), // Command input
                Constraint::Length(3), // Status
            ])
            .split(f.area());

        self.render_display_area(f, chunks[0]);
        self.render_command_input(f, chunks[1]);
        self.render_status(f, chunks[2]);
    }

    fn render_display_area(&mut self, f: &mut Frame, area: Rect) {
        let visible_height = area.height as usize - 2; // Subtract borders
        self.last_visible_height = visible_height; // Track for scroll calculations

        // Update current terminal width
        self.current_terminal_width = area.width as usize;

        // Show welcome message if display is empty
        if self.display_content.is_empty() {
            self.show_welcome(self.current_terminal_width);
        }

        // Auto-scroll to bottom unless user is manually scrolling
        if !self.manual_scroll_mode {
            self.auto_scroll_to_bottom(visible_height);
        }

        let mut lines = Vec::new();

        // Add existing display content with styling
        for line in &self.display_content {
            lines.push(self.parse_styled_line(line));
        }

        // Add completions if available
        if !self.completions.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from("Completions:"));

            for completion in &self.completions {
                let prefix_len = self.completion_prefix.len();
                if completion.len() >= prefix_len {
                    // Split completion into typed (green) and untyped (gray) parts
                    let typed_part = &completion[..prefix_len];
                    let untyped_part = &completion[prefix_len..];

                    let mut spans = vec![Span::styled(
                        typed_part,
                        Style::default().fg(Color::Green).bold(),
                    )];

                    if !untyped_part.is_empty() {
                        spans.push(Span::styled(untyped_part, Style::default().fg(Color::Gray)));
                    }

                    lines.push(Line::from(spans));
                } else {
                    lines.push(Line::from(completion.as_str()));
                }
            }
        }

        let display_lines: Vec<Line> = lines
            .into_iter()
            .skip(self.display_scroll)
            .take(area.height as usize - 2)
            .collect();

        // Show scroll indicator in title if in manual scroll mode
        let title = if self.manual_scroll_mode {
            "Display (PgUp/PgDn scroll, End = auto-scroll)"
        } else {
            "Display"
        };

        let display_paragraph = Paragraph::new(display_lines)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false });

        f.render_widget(display_paragraph, area);
    }

    fn render_command_input(&self, f: &mut Frame, area: Rect) {
        // Calculate the available width for text (subtract borders)
        let text_width = area.width.saturating_sub(2) as usize;

        // Calculate horizontal scroll offset to keep cursor visible
        let (visible_text, cursor_offset, is_scrolled) = self.calculate_visible_text(text_width);

        // Show different title and style based on command state
        let (title, border_style) = if self.cancellation_token.is_some() {
            // Command is running - show blocked state
            (
                "Command (Running - Ctrl-C to cancel)",
                Style::default().fg(Color::Red),
            )
        } else if is_scrolled {
            // Normal state but text is scrolled
            ("Command ◄►", Style::default().fg(Color::Yellow))
        } else {
            // Normal state
            ("Command", Style::default().fg(Color::Yellow))
        };

        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style);

        let input_text = Paragraph::new(visible_text)
            .style(Style::default().fg(Color::White))
            .block(input_block);

        f.render_widget(input_text, area);

        // Position cursor relative to the visible text
        f.set_cursor_position(Position {
            x: area.x + cursor_offset as u16 + 1,
            y: area.y + 1,
        });
    }

    fn calculate_visible_text(&self, text_width: usize) -> (String, usize, bool) {
        if self.command_input.len() <= text_width {
            // Command fits entirely - no scrolling needed
            return (self.command_input.clone(), self.command_cursor, false);
        }

        // Command is longer than available width - need to scroll
        let cursor_pos = self.command_cursor;

        // Calculate scroll offset to keep cursor visible
        let scroll_offset = if cursor_pos < text_width {
            // Cursor is near the beginning
            0
        } else {
            // Keep cursor in the last quarter of visible area for smooth scrolling
            cursor_pos.saturating_sub(text_width * 3 / 4)
        };

        let end_pos = (scroll_offset + text_width).min(self.command_input.len());
        let visible_text = self.command_input[scroll_offset..end_pos].to_string();
        let cursor_offset = cursor_pos.saturating_sub(scroll_offset);

        (visible_text, cursor_offset, true)
    }

    fn render_status(&self, f: &mut Frame, area: Rect) {
        let status_paragraph = Paragraph::new(self.status_message.as_str())
            .style(Style::default().fg(Color::Green))
            .block(Block::default().borders(Borders::ALL).title("Status"));

        f.render_widget(status_paragraph, area);
    }

    // Event handlers
    pub fn handle_ctrl_c(&mut self) {
        if let Some(token) = &self.cancellation_token {
            // Command is running - cancel it
            token.cancel();
            self.display_content
                .push("^C - Command cancelled".to_string());
            self.status_message = "Command cancelled".to_string();

            // Clean up receivers
            self.cancellation_token = None;
            self.progress_receiver = None;
            self.result_receiver = None;
        } else {
            // No command running - clear input
            self.command_input.clear();
            self.command_cursor = 0;
            self.completions.clear();
            self.history_index = None;
            self.temp_input.clear();
            self.status_message = "Input cleared".to_string();
        }
    }

    /// Process any pending async events (progress updates, command completion)
    pub fn process_async_events(&mut self) {
        // Process progress messages
        if let Some(receiver) = &mut self.progress_receiver {
            while let Ok(message) = receiver.try_recv() {
                self.display_content.push(format!("  {}", message));
            }
        }

        // Check for command completion
        if let Some(receiver) = &mut self.result_receiver {
            if let Ok(result) = receiver.try_recv() {
                // Command completed
                self.cancellation_token = None;
                self.progress_receiver = None;
                self.result_receiver = None;

                match result {
                    Ok(query_result) => {
                        let formatted_result = self
                            .analytics_engine
                            .format_result_for_display(&query_result, Some(100));
                        self.display_content.extend(formatted_result);
                        self.current_result = Some(query_result.clone());
                        self.status_message = format!(
                            "Query completed: {} rows in {}",
                            query_result.row_count,
                            crate::analytics::AnalyticsEngine::format_execution_time(
                                query_result.execution_time_ms
                            )
                        );
                    }
                    Err(e) => {
                        self.display_content.push(format!("Error: {}", e));
                        self.status_message = "Query failed".to_string();
                    }
                }
            }
        }
    }

    pub fn handle_escape(&mut self) {
        // Don't allow escape while command is running
        if self.cancellation_token.is_some() {
            return;
        }

        // Clear completions and reset command input
        self.completions.clear();
        self.command_input.clear();
        self.command_cursor = 0;
        self.status_message = "Command cleared".to_string();
    }

    pub fn handle_enter(&mut self) -> Result<bool> {
        // Don't allow new commands while one is already running
        if self.cancellation_token.is_some() {
            self.status_message = "Command already running - press Ctrl-C to cancel".to_string();
            return Ok(false);
        }

        let command = self.command_input.trim().to_string();
        if command.is_empty() {
            return Ok(false); // false = don't quit
        }

        // Add to command history (avoid duplicates of the last command)
        if self.command_history.last() != Some(&command) {
            self.command_history.push(command.clone());
        }

        // Reset history navigation
        self.history_index = None;
        self.temp_input.clear();

        // Clear completions
        self.completions.clear();

        // Add command to display history with blank line separator
        self.display_content.push("".to_string());
        self.display_content.push(format!("CMD_PROMPT>{}", command));

        // Handle special commands
        if command == "quit" || command == "exit" {
            return Ok(true); // true = quit requested
        }

        if command == "help" {
            self.show_help();
            self.command_input.clear();
            self.command_cursor = 0;
            return Ok(false);
        }

        if command == "list" {
            self.show_list(self.current_terminal_width);
            self.command_input.clear();
            self.command_cursor = 0;
            return Ok(false);
        }

        if command.starts_with("export ") {
            let filename = command.strip_prefix("export ").unwrap().trim();
            self.handle_export(filename);
            self.command_input.clear();
            self.command_cursor = 0;
            return Ok(false);
        }

        if command.starts_with("describe ") || command.starts_with("DESCRIBE ") {
            let table_name = command.split_whitespace().nth(1).unwrap_or("").trim();
            if !table_name.is_empty() {
                self.spawn_describe_command(table_name.to_string());
            } else {
                self.display_content
                    .push("Usage: DESCRIBE <table_name>".to_string());
            }
            self.command_input.clear();
            self.command_cursor = 0;
            return Ok(false);
        }

        // Parse and execute query
        let query_type = if self.is_function_call(&command) {
            match QueryEngine::parse_function_call(&command) {
                Ok((function_name, parameters)) => QueryType::Function(function_name, parameters),
                Err(e) => {
                    self.display_content
                        .push(format!("Function parsing error: {}", e));
                    self.command_input.clear();
                    self.command_cursor = 0;
                    return Ok(false);
                }
            }
        } else {
            QueryType::RawSql(command.to_string())
        };

        // Spawn command execution
        self.spawn_query_execution(query_type);

        // Clear command input
        self.command_input.clear();
        self.command_cursor = 0;

        Ok(false) // false = don't quit
    }

    fn spawn_query_execution(&mut self, query_type: QueryType) {
        self.status_message = "Executing query...".to_string();

        // Create cancellation token for this command
        let cancellation_token = tokio_util::sync::CancellationToken::new();
        self.cancellation_token = Some(cancellation_token.clone());

        // Create channels for progress and results
        let (progress_sender, progress_receiver) = tokio::sync::mpsc::unbounded_channel();
        let (result_sender, result_receiver) = tokio::sync::oneshot::channel();

        self.progress_receiver = Some(progress_receiver);
        self.result_receiver = Some(result_receiver);

        let progress_callback = std::sync::Arc::new(move |message: String| {
            let _ = progress_sender.send(message);
        }) as crate::query_engine::functions::ProgressCallback;

        // Clone what we need for the spawned task
        let analytics_engine = Arc::clone(&self.analytics_engine);

        // Spawn the command execution
        tokio::spawn(async move {
            tokio::select! {
                // Handle cancellation
                _ = cancellation_token.cancelled() => {
                    // Command was cancelled - just exit
                    let _ = result_sender.send(Err(anyhow::anyhow!("Command cancelled")));
                }

                // Execute the query
                result = analytics_engine.execute_query(query_type, progress_callback) => {
                    let _ = result_sender.send(result);
                }
            }
        });
    }

    fn spawn_describe_command(&mut self, table_name: String) {
        self.status_message = "Describing table...".to_string();

        // Create cancellation token for this command
        let cancellation_token = tokio_util::sync::CancellationToken::new();
        self.cancellation_token = Some(cancellation_token.clone());

        // Create channels for results (no progress needed for simple describe)
        let (result_sender, result_receiver) = tokio::sync::oneshot::channel();

        self.progress_receiver = None; // No progress for describe command
        self.result_receiver = Some(result_receiver);

        // Clone what we need for the spawned task
        let query_engine = Arc::clone(&self.query_engine);

        // Spawn the describe command execution
        tokio::spawn(async move {
            tokio::select! {
                // Handle cancellation
                _ = cancellation_token.cancelled() => {
                    let _ = result_sender.send(Err(anyhow::anyhow!("Command cancelled")));
                }

                // Execute the describe query using DataFusion schema info
                result = async {
                    let ctx = query_engine.get_context();
                    match ctx.table(&table_name).await {
                        Ok(table) => {
                            let schema = table.schema();
                            let columns = vec!["Column".to_string(), "Type".to_string(), "Nullable".to_string()];
                            let mut rows = Vec::new();

                            for field in schema.fields() {
                                let nullable = if field.is_nullable() { "YES" } else { "NO" };
                                rows.push(vec![
                                    field.name().clone(),
                                    format!("{:?}", field.data_type()),
                                    nullable.to_string()
                                ]);
                            }

                            Ok((columns, rows))
                        }
                        Err(e) => Err(anyhow::anyhow!("Table '{}' not found: {}", table_name, e))
                    }
                } => {
                    let query_result = match result {
                        Ok((columns, rows)) => {
                            let row_count = rows.len();
                            Ok(QueryResult {
                                columns,
                                rows,
                                row_count,
                                execution_time_ms: 0,
                            })
                        }
                        Err(e) => Err(e)
                    };
                    let _ = result_sender.send(query_result);
                }
            }
        });
    }

    pub fn is_input_empty(&self) -> bool {
        self.command_input.trim().is_empty()
    }

    fn auto_scroll_to_bottom(&mut self, visible_height: usize) {
        // Calculate total lines including completions
        let total_lines = self.get_total_display_lines();

        // If content fits in visible area, no scrolling needed
        if total_lines <= visible_height {
            self.display_scroll = 0;
        } else {
            // Scroll to show the bottom content
            self.display_scroll = total_lines.saturating_sub(visible_height);
        }
    }

    fn get_total_display_lines(&self) -> usize {
        let mut line_count = self.display_content.len();

        // Add completion lines if visible
        if !self.completions.is_empty() {
            line_count += 2; // Empty line + "Completions:" header
            line_count += self.completions.len();
        }

        line_count
    }

    fn parse_styled_line(&self, line: &str) -> Line<'_> {
        if let Some(command) = line.strip_prefix("CMD_PROMPT>") {
            // Create a styled line with bright green ">" and normal text for command
            Line::from(vec![
                Span::styled(">", Style::default().fg(Color::Green).bold()),
                Span::raw(" "),
                Span::raw(command.to_string()),
            ])
        } else {
            // Regular line, no special styling
            Line::from(line.to_string())
        }
    }

    fn show_help(&mut self) {
        use crate::query_engine::functions;

        self.display_content.extend(vec![
            "".to_string(),
            "Analytics Shell Help".to_string(),
            "===================".to_string(),
            "".to_string(),
            "Query Types:".to_string(),
            "• SQL: Any SELECT statement".to_string(),
            "• Functions: function_name or function_name(param=\"value\")".to_string(),
            "".to_string(),
            "Available Functions:".to_string(),
        ]);

        // Dynamically add all registered functions
        for func in functions::get_registered_functions() {
            self.display_content
                .push(format!("• {}", func.usage_example()));
            self.display_content
                .push(format!("  {}", func.description()));
        }

        self.display_content.extend(vec![
            "".to_string(),
            "Commands:".to_string(),
            "• help - Show this help".to_string(),
            "• list - List all available functions and tables".to_string(),
            "• export filename.csv - Export last result to CSV".to_string(),
            "• describe table_name - Show table structure and columns".to_string(),
            "• quit or exit - Quit the shell".to_string(),
            "".to_string(),
            "Keyboard Shortcuts:".to_string(),
            "• Tab - Show completions".to_string(),
            "• Enter - Execute command".to_string(),
            "• Esc - Clear command".to_string(),
            "• Up/Down - Navigate command history".to_string(),
            "• PgUp/PgDn - Scroll display area".to_string(),
            "• Mouse wheel - Scroll display area".to_string(),
            "• Shift+mouse - Select text for copying".to_string(),
            "• Home/End - Jump to previous command/bottom in display".to_string(),
            "• Ctrl-C - Cancel running command or clear input".to_string(),
            "• Ctrl-D - Quit when input is empty".to_string(),
            "".to_string(),
        ]);
    }

    fn handle_export(&mut self, filename: &str) {
        if let Some(ref result) = self.current_result {
            match self.analytics_engine.export_to_csv(result, filename) {
                Ok(_) => {
                    self.display_content.push(format!(
                        "Exported {} rows to {}",
                        result.row_count, filename
                    ));
                    self.status_message = format!("Export successful: {}", filename);
                }
                Err(e) => {
                    self.display_content.push(format!("Export failed: {}", e));
                    self.status_message = "Export failed".to_string();
                }
            }
        } else {
            self.display_content
                .push("No query result to export".to_string());
            self.status_message = "No result to export".to_string();
        }
    }

    pub fn handle_tab(&mut self) {
        // Don't allow tab completion while command is running
        if self.cancellation_token.is_some() {
            return;
        }

        self.update_completions();

        if self.completions.is_empty() {
            return;
        }

        // Find the longest common prefix among all completions
        let common_prefix = self.find_common_prefix();

        if common_prefix.len() > self.completion_prefix.len() {
            // Replace the current word with the extended prefix
            let input = &self.command_input[..self.command_cursor];
            let last_word_start = input.rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0);

            // Remove the current partial word and insert the completion
            let before_word = &self.command_input[..last_word_start];
            let after_cursor = &self.command_input[self.command_cursor..];

            self.command_input = format!("{}{}{}", before_word, common_prefix, after_cursor);
            self.command_cursor = last_word_start + common_prefix.len();

            // Update completions with the new input
            self.update_completions();
        }

        // If there's only one completion and it's fully matched, clear completions
        if self.completions.len() == 1 && self.completions[0] == common_prefix {
            self.completions.clear();
        }
    }

    pub fn handle_up(&mut self) {
        // Don't allow history navigation while command is running
        if self.cancellation_token.is_some() {
            return;
        }

        // Navigate command history up (older commands)
        if self.command_history.is_empty() {
            return;
        }

        match self.history_index {
            None => {
                // Starting history navigation - save current input
                self.temp_input = self.command_input.clone();
                self.history_index = Some(self.command_history.len() - 1);
                self.load_history_command();
            }
            Some(index) => {
                if index > 0 {
                    self.history_index = Some(index - 1);
                    self.load_history_command();
                }
            }
        }
    }

    pub fn handle_down(&mut self) {
        // Don't allow history navigation while command is running
        if self.cancellation_token.is_some() {
            return;
        }

        // Navigate command history down (newer commands)
        if let Some(index) = self.history_index {
            if index < self.command_history.len() - 1 {
                self.history_index = Some(index + 1);
                self.load_history_command();
            } else {
                // Reached the end - restore original input
                self.history_index = None;
                self.command_input = self.temp_input.clone();
                self.command_cursor = self.command_input.len();
            }
        }
    }

    fn load_history_command(&mut self) {
        if let Some(index) = self.history_index {
            if let Some(command) = self.command_history.get(index) {
                self.command_input = command.clone();
                self.command_cursor = self.command_input.len();
            }
        }
    }

    pub fn handle_left(&mut self) {
        // Don't allow cursor movement while command is running
        if self.cancellation_token.is_some() {
            return;
        }

        if self.command_cursor > 0 {
            self.command_cursor -= 1;
        }
    }

    pub fn handle_right(&mut self) {
        // Don't allow cursor movement while command is running
        if self.cancellation_token.is_some() {
            return;
        }

        if self.command_cursor < self.command_input.len() {
            self.command_cursor += 1;
        }
    }

    pub fn handle_page_up(&mut self) {
        // Enable manual scroll mode and scroll up
        self.manual_scroll_mode = true;
        let scroll_amount = 10; // Scroll 10 lines at a time
        self.display_scroll = self.display_scroll.saturating_sub(scroll_amount);
    }

    pub fn handle_page_down(&mut self) {
        let total_lines = self.get_total_display_lines();
        let scroll_amount = 10; // Scroll 10 lines at a time
        let visible_height = self.last_visible_height;

        // Scroll down
        self.display_scroll += scroll_amount;
        self.manual_scroll_mode = true;

        // Check if we've scrolled to or past the bottom after scrolling
        if self.display_scroll + visible_height >= total_lines {
            // We've reached the bottom - exit manual mode and let auto-scroll take over
            self.manual_scroll_mode = false;
        }
    }

    pub fn handle_home(&mut self) {
        // Jump to the command whose output overlaps the top of the display
        self.manual_scroll_mode = true;

        // Find the command strictly before the current scroll position
        let target_line = self.find_command_before_line(self.display_scroll);
        self.display_scroll = target_line;
    }

    fn find_command_before_line(&self, line_index: usize) -> usize {
        // Search backwards from before the given line to find the most recent command prompt
        if line_index == 0 {
            return 0; // Already at the beginning
        }

        // Start searching from line_index - 1 (strictly before)
        for i in (0..line_index).rev() {
            if self.display_content[i].starts_with("CMD_PROMPT>") {
                return i;
            }
        }

        // If no command found, go to the beginning
        0
    }

    pub fn handle_end(&mut self) {
        // Jump straight to bottom and resume auto-scroll mode
        self.manual_scroll_mode = false;
        // scroll position will be set by auto_scroll_to_bottom on next render
    }

    pub fn handle_mouse_scroll_up(&mut self) {
        // Mouse scroll up = scroll display content up (show earlier content)
        self.manual_scroll_mode = true;
        let scroll_amount = 3; // Scroll 3 lines at a time for smooth scrolling
        self.display_scroll = self.display_scroll.saturating_sub(scroll_amount);
    }

    pub fn handle_mouse_scroll_down(&mut self) {
        // Mouse scroll down = scroll display content down (show later content)
        let total_lines = self.get_total_display_lines();
        let scroll_amount = 3; // Scroll 3 lines at a time for smooth scrolling
        let visible_height = self.last_visible_height;

        // Scroll down
        self.display_scroll += scroll_amount;
        self.manual_scroll_mode = true;

        // Check if we've scrolled to or past the bottom after scrolling
        if self.display_scroll + visible_height >= total_lines {
            // We've reached the bottom - exit manual mode and let auto-scroll take over
            self.manual_scroll_mode = false;
        }
    }

    pub fn handle_char(&mut self, c: char) {
        // Don't allow input while command is running
        if self.cancellation_token.is_some() {
            return;
        }

        // Reset history navigation when user types
        self.reset_history_navigation();

        self.command_input.insert(self.command_cursor, c);
        self.command_cursor += 1;
        self.update_completions();
    }

    pub fn handle_backspace(&mut self) {
        // Don't allow input while command is running
        if self.cancellation_token.is_some() {
            return;
        }

        if self.command_cursor > 0 {
            // Reset history navigation when user edits
            self.reset_history_navigation();

            self.command_input.remove(self.command_cursor - 1);
            self.command_cursor -= 1;
            self.update_completions();
        }
    }

    fn reset_history_navigation(&mut self) {
        self.history_index = None;
        self.temp_input.clear();
    }

    fn update_completions(&mut self) {
        // Find the word being completed (from cursor position backward to word boundary)
        let input = &self.command_input[..self.command_cursor];
        let last_word_start = input.rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0);
        let current_word = &input[last_word_start..];

        if current_word.is_empty() {
            self.completions.clear();
            self.completion_prefix = String::new();
            return;
        }

        self.completion_prefix = current_word.to_string();
        let mut completions = Vec::new();

        // Function completions
        for func in self.query_engine.get_available_functions() {
            if func.starts_with(current_word) {
                completions.push(func);
            }
        }

        // Use analytics engine for comprehensive suggestions (includes SQL keywords, commands, and tables)
        let suggestions = self.analytics_engine.get_query_suggestions(current_word);
        completions.extend(suggestions);

        // Remove duplicates and sort
        completions.sort();
        completions.dedup();
        self.completions = completions;
    }

    fn find_common_prefix(&self) -> String {
        if self.completions.is_empty() {
            return String::new();
        }

        if self.completions.len() == 1 {
            return self.completions[0].clone();
        }

        let first = &self.completions[0];
        let mut common_len = first.len();

        for completion in &self.completions[1..] {
            let mut chars1 = first.chars();
            let mut chars2 = completion.chars();
            let mut current_len = 0;

            while let (Some(c1), Some(c2)) = (chars1.next(), chars2.next()) {
                if c1 == c2 {
                    current_len += c1.len_utf8();
                } else {
                    break;
                }
            }

            common_len = common_len.min(current_len);
        }

        first[..common_len].to_string()
    }

    fn is_function_call(&self, query: &str) -> bool {
        // Check if it's a function call syntax or a simple function name
        if let Ok((function_name, _)) = QueryEngine::parse_function_call(query) {
            self.query_engine
                .get_available_functions()
                .contains(&function_name)
        } else {
            false
        }
    }
}
