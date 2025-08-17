use anyhow::Result;
use ratatui::{
    prelude::*,
    widgets::*,
};

use crate::analytics::AnalyticsEngine;
use crate::query_engine::{QueryEngine, QueryType, QueryResult};

pub struct App {
    query_engine: QueryEngine,
    analytics_engine: AnalyticsEngine,
    debug: bool,
    
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
    temp_input: String, // Stores current input when browsing history
    
    // Status
    status_message: String,
    
    // Track actual visible height for accurate scroll calculations
    last_visible_height: usize,
}

impl App {
    pub fn new(query_engine: QueryEngine, analytics_engine: AnalyticsEngine, debug: bool) -> Self {
        let mut app = Self {
            query_engine,
            analytics_engine,
            debug,
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
            status_message: "Welcome to Analytics Shell! Type a command and press Enter.".to_string(),
            last_visible_height: 20, // Default value
        };
        
        // Show initial help in display area
        app.show_welcome();
        app
    }
    
    fn show_welcome(&mut self) {
        self.display_content = vec![
            "Analytics Shell - Interactive Query Interface".to_string(),
            "".to_string(),
            "Available Commands:".to_string(),
            "• Functions: clickthrough_analytics, join_analytics".to_string(),
            "• SQL: SELECT * FROM activity_log LIMIT 10".to_string(),
            "• Special: help, export filename.csv, describe table_name".to_string(),
            "".to_string(),
            "Tab completion is available for functions and SQL keywords.".to_string(),
            "Use ↑/↓ arrow keys to navigate command history.".to_string(),
            "Type 'help' for detailed information.".to_string(),
            "".to_string(),
        ];
    }

    pub fn render(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),     // Display area
                Constraint::Length(3),  // Command input
                Constraint::Length(3),  // Status
            ])
            .split(f.area());

        self.render_display_area(f, chunks[0]);
        self.render_command_input(f, chunks[1]);
        self.render_status(f, chunks[2]);
    }

    fn render_display_area(&mut self, f: &mut Frame, area: Rect) {
        let visible_height = area.height as usize - 2; // Subtract borders
        self.last_visible_height = visible_height; // Track for scroll calculations
        
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
                    
                    let mut spans = vec![
                        Span::styled(typed_part, Style::default().fg(Color::Green).bold()),
                    ];
                    
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
            .wrap(Wrap { trim: true });

        f.render_widget(display_paragraph, area);
    }

    fn render_command_input(&self, f: &mut Frame, area: Rect) {
        // Calculate the available width for text (subtract borders)
        let text_width = area.width.saturating_sub(2) as usize;
        
        // Calculate horizontal scroll offset to keep cursor visible
        let (visible_text, cursor_offset, is_scrolled) = self.calculate_visible_text(text_width);
        
        // Show scroll indicator in title if text is scrolled
        let title = if is_scrolled {
            "Command ◄►"  // Unicode arrows to indicate scrollable content
        } else {
            "Command"
        };

        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Yellow));

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
    pub fn handle_escape(&mut self) {
        // Clear completions and reset command input
        self.completions.clear();
        self.command_input.clear();
        self.command_cursor = 0;
        self.status_message = "Command cleared".to_string();
    }

    pub async fn handle_enter(&mut self) -> Result<bool> {
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
                self.handle_describe_table(table_name).await?;
            } else {
                self.display_content.push("Usage: DESCRIBE <table_name>".to_string());
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
                    self.display_content.push(format!("Function parsing error: {}", e));
                    self.command_input.clear();
                    self.command_cursor = 0;
                    return Ok(false);
                }
            }
        } else {
            QueryType::RawSql(command.to_string())
        };

        self.status_message = "Executing query...".to_string();

        match self.analytics_engine.execute_query(query_type).await {
            Ok(result) => {
                let formatted_result = self.analytics_engine.format_result_for_display(&result, Some(100));
                self.display_content.extend(formatted_result);
                self.current_result = Some(result.clone());
                self.status_message = format!(
                    "Query completed: {} rows in {}ms",
                    result.row_count,
                    result.execution_time_ms
                );
            }
            Err(e) => {
                self.display_content.push(format!("Error: {}", e));
                self.status_message = "Query failed".to_string();
            }
        }

        // Clear command input
        self.command_input.clear();
        self.command_cursor = 0;
        
        Ok(false) // false = don't quit
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

    fn parse_styled_line(&self, line: &str) -> Line {
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
            self.display_content.push(format!("• {}", func.usage_example()));
            self.display_content.push(format!("  {}", func.description()));
        }
        
        self.display_content.extend(vec![
            "".to_string(),
            "Commands:".to_string(),
            "• help - Show this help".to_string(),
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
            "• Home/End - Jump to top/bottom of display".to_string(),
            "• Ctrl-D - Quit when input is empty".to_string(),
            "".to_string(),
        ]);
    }

    fn handle_export(&mut self, filename: &str) {
        if let Some(ref result) = self.current_result {
            match self.analytics_engine.export_to_csv(result, filename) {
                Ok(_) => {
                    self.display_content.push(format!("Exported {} rows to {}", result.row_count, filename));
                    self.status_message = format!("Export successful: {}", filename);
                }
                Err(e) => {
                    self.display_content.push(format!("Export failed: {}", e));
                    self.status_message = "Export failed".to_string();
                }
            }
        } else {
            self.display_content.push("No query result to export".to_string());
            self.status_message = "No result to export".to_string();
        }
    }

    async fn handle_describe_table(&mut self, table_name: &str) -> Result<()> {
        // Use DataFusion to get schema information
        let ctx = self.query_engine.get_context();
        
        // Try to get table schema from DataFusion catalog
        match ctx.table(table_name).await {
            Ok(table) => {
                let schema = table.schema();
                
                self.display_content.push("".to_string());
                self.display_content.push(format!("Table: {}", table_name));
                self.display_content.push("─".repeat(50));
                self.display_content.push(format!("{:<20} {:<15} {:<10}", "Column", "Type", "Nullable"));
                self.display_content.push("─".repeat(50));
                
                for field in schema.fields() {
                    let nullable = if field.is_nullable() { "YES" } else { "NO" };
                    self.display_content.push(format!("{:<20} {:<15} {:<10}", 
                        field.name(), 
                        format!("{:?}", field.data_type()),
                        nullable
                    ));
                }
                
                self.display_content.push("─".repeat(50));
                self.display_content.push(format!("Total columns: {}", schema.fields().len()));
                self.status_message = format!("Described table: {}", table_name);
            }
            Err(_) => {
                self.display_content.push(format!("Table '{}' not found in catalog", table_name));
                self.display_content.push("".to_string());
                self.display_content.push("Available tables:".to_string());
                
                // List available tables
                let available_tables = vec![
                    "activity_log", "page_views", "clickthroughs", "user_tags",
                    "participation", "bookmark", "join_table",
                    "clickthrough_daily_summary", "clickthrough_weekly_summary",
                    "join_daily_summary", "join_weekly_summary",
                    "user_tags_cache", "user_emails_cache"
                ];
                
                for table in available_tables {
                    self.display_content.push(format!("  • {}", table));
                }
                
                self.status_message = format!("Table '{}' not found", table_name);
            }
        }
        
        Ok(())
    }

    pub fn handle_tab(&mut self) {
        self.update_completions();
        
        if self.completions.is_empty() {
            return;
        }
        
        // Find the longest common prefix among all completions
        let common_prefix = self.find_common_prefix();
        
        if common_prefix.len() > self.completion_prefix.len() {
            // Replace the current word with the extended prefix
            let input = &self.command_input[..self.command_cursor];
            let last_word_start = input.rfind(char::is_whitespace)
                .map(|i| i + 1)
                .unwrap_or(0);
            
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
        if self.command_cursor > 0 {
            self.command_cursor -= 1;
        }
    }

    pub fn handle_right(&mut self) {
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
        // Jump to top of content
        self.manual_scroll_mode = true;
        self.display_scroll = 0;
    }

    pub fn handle_end(&mut self) {
        // Jump straight to bottom and resume auto-scroll mode
        self.manual_scroll_mode = false;
        // scroll position will be set by auto_scroll_to_bottom on next render
    }

    pub fn handle_char(&mut self, c: char) {
        // Reset history navigation when user types
        self.reset_history_navigation();
        
        self.command_input.insert(self.command_cursor, c);
        self.command_cursor += 1;
        self.update_completions();
    }

    pub fn handle_backspace(&mut self) {
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
        let last_word_start = input.rfind(char::is_whitespace)
            .map(|i| i + 1)
            .unwrap_or(0);
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
            self.query_engine.get_available_functions().contains(&function_name)
        } else {
            false
        }
    }
}