use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use scraper::Html;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delta {
    pub ops: Vec<Op>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Op {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert: Option<Insert>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Insert {
    Text(String),
    Embed(HashMap<String, serde_json::Value>),
}

/// Convert Quill Delta JSON to HTML
pub fn delta_to_html(delta_json: &str) -> Result<String, anyhow::Error> {
    let delta: Delta = serde_json::from_str(delta_json)?;
    Ok(delta_to_html_impl(&delta))
}

/// Extract plain text excerpt from Delta JSON
/// Removes all formatting and limits to specified length
pub fn delta_to_excerpt(delta_json: &str, max_len: usize) -> String {
    if let Ok(delta) = serde_json::from_str::<Delta>(delta_json) {
        let text: String = delta
            .ops
            .iter()
            .filter_map(|op| {
                if let Some(Insert::Text(s)) = &op.insert {
                    Some(s.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("");

        let trimmed = text.trim();
        if trimmed.chars().count() > max_len {
            let truncated: String = trimmed.chars().take(max_len).collect();
            format!("{}...", truncated)
        } else {
            trimmed.to_string()
        }
    } else {
        String::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum BlockType {
    None,
    Paragraph,
    Header(u64),
    Blockquote,
    CodeBlock,
    OrderedList,
    BulletList,
    RowItem,
    ColumnItem,
}

fn delta_to_html_impl(delta: &Delta) -> String {
    let mut html = String::new();
    let mut line_content = String::new();
    let mut current_list_type = BlockType::None;

    for op in &delta.ops {
        if let Some(Insert::Text(text)) = &op.insert {
            // Check if this is a standalone newline with block-level attributes
            if text == "\n" {
                // Determine block type from attributes
                let block_type = if let Some(attrs) = &op.attributes {
                    get_block_type_from_attrs(attrs)
                } else {
                    BlockType::Paragraph
                };
                output_line(
                    &mut html,
                    &mut line_content,
                    block_type,
                    &mut current_list_type,
                );
            } else if text.contains('\n') {
                // Text contains embedded newlines - split and process
                let parts: Vec<&str> = text.split('\n').collect();

                for (i, part) in parts.iter().enumerate() {
                    // Add the text content (if not empty)
                    if !part.is_empty() {
                        let formatted = if let Some(attrs) = &op.attributes {
                            apply_inline_formatting(part, attrs)
                        } else {
                            html_escape(part)
                        };
                        line_content.push_str(&formatted);
                    }

                    // If this is not the last part, we hit a newline
                    if i < parts.len() - 1 {
                        // Embedded newlines are treated as paragraph breaks
                        output_line(
                            &mut html,
                            &mut line_content,
                            BlockType::Paragraph,
                            &mut current_list_type,
                        );
                    }
                }
            } else {
                // No newline in this text, just accumulate content
                let formatted = if let Some(attrs) = &op.attributes {
                    apply_inline_formatting(text, attrs)
                } else {
                    html_escape(text)
                };
                line_content.push_str(&formatted);
            }
        } else if let Some(Insert::Embed(embed)) = &op.insert {
            // Embeds like images and figures - close current line first
            if !line_content.is_empty() {
                output_line(
                    &mut html,
                    &mut line_content,
                    BlockType::Paragraph,
                    &mut current_list_type,
                );
            }

            // Handle regular image embeds
            if let Some(image_url) = embed.get("image") {
                if let Some(url) = image_url.as_str().and_then(sanitize_url) {
                    html.push_str(&format!("<img src=\"{}\" />", html_escape(url)));
                }
            }

            // Handle button embeds (a link drawn as a call-to-action button)
            if let Some(button) = embed.get("button").and_then(|v| v.as_object()) {
                let url = button
                    .get("url")
                    .and_then(|v| v.as_str())
                    .and_then(sanitize_url)
                    .unwrap_or("");
                let label = button.get("label").and_then(|v| v.as_str()).unwrap_or("");

                // A button that leads nowhere or says nothing is not drawn.
                if !url.is_empty() && !label.trim().is_empty() {
                    html.push_str(&button_html(url, label));
                }
            }

            // Handle figure embeds (images with captions)
            if let Some(figure_data) = embed.get("figure") {
                if let Some(obj) = figure_data.as_object() {
                    let url = obj
                        .get("url")
                        .and_then(|v| v.as_str())
                        .and_then(sanitize_url)
                        .unwrap_or("");
                    let caption = obj.get("caption").and_then(|v| v.as_str()).unwrap_or("");
                    let align = obj
                        .get("align")
                        .and_then(|v| v.as_str())
                        .unwrap_or("center");
                    let width = obj.get("width").and_then(|v| v.as_str()).unwrap_or("");

                    let figure_class = get_figure_class(align);
                    html.push_str(&format!("<figure class=\"{}\">", figure_class));

                    // Add width style if specified
                    if !width.is_empty() {
                        html.push_str(&format!(
                            "<img src=\"{}\" alt=\"{}\" style=\"width: {}\" />",
                            html_escape(url),
                            html_escape(caption),
                            html_escape(width)
                        ));
                    } else {
                        html.push_str(&format!(
                            "<img src=\"{}\" alt=\"{}\" />",
                            html_escape(url),
                            html_escape(caption)
                        ));
                    }

                    if !caption.is_empty() {
                        html.push_str(&format!(
                            "<figcaption>{}</figcaption>",
                            html_escape(caption)
                        ));
                    }
                    html.push_str("</figure>");
                }
            }
        }
    }

    // Close any remaining line content
    if !line_content.is_empty() {
        output_line(
            &mut html,
            &mut line_content,
            BlockType::Paragraph,
            &mut current_list_type,
        );
    }

    // Close any remaining list
    if current_list_type != BlockType::None {
        close_list(&mut html, current_list_type);
    }

    html
}

fn output_line(
    html: &mut String,
    line_content: &mut String,
    block_type: BlockType,
    current_list_type: &mut BlockType,
) {
    match block_type {
        BlockType::OrderedList
        | BlockType::BulletList
        | BlockType::RowItem
        | BlockType::ColumnItem => {
            // Handle list items and row/column items
            if *current_list_type != block_type {
                // Close previous list/container if different type
                if *current_list_type != BlockType::None {
                    close_list(html, *current_list_type);
                }
                // Open new list/container
                open_list(html, block_type);
                *current_list_type = block_type;
            }

            // Output the item
            match block_type {
                BlockType::OrderedList | BlockType::BulletList => {
                    // Skip empty list items
                    if line_content.is_empty() {
                        return;
                    }
                    html.push_str("<li>");
                    html.push_str(line_content);
                    html.push_str("</li>");
                }
                BlockType::RowItem => {
                    html.push_str("<div class=\"flex-1 min-w-0\">");
                    if line_content.is_empty() {
                        html.push_str("<br>");
                    } else {
                        html.push_str(line_content);
                    }
                    html.push_str("</div>");
                }
                BlockType::ColumnItem => {
                    html.push_str("<div class=\"flex-1\">");
                    if line_content.is_empty() {
                        html.push_str("<br>");
                    } else {
                        html.push_str(line_content);
                    }
                    html.push_str("</div>");
                }
                _ => {}
            }
        }
        _ => {
            // Close any open list/container
            if *current_list_type != BlockType::None {
                close_list(html, *current_list_type);
                *current_list_type = BlockType::None;
            }

            // Output regular block (even if empty - creates spacing)
            if let Some(opening) = open_block_tag(block_type) {
                html.push_str(&opening);
            }

            // Add <br> to empty blocks to ensure they render with height
            if line_content.is_empty() {
                html.push_str("<br>");
            } else {
                html.push_str(line_content);
            }

            if let Some(closing) = close_block_tag(block_type) {
                html.push_str(&closing);
            }
        }
    }

    line_content.clear();
}

fn open_list(html: &mut String, block_type: BlockType) {
    match block_type {
        BlockType::OrderedList => {
            html.push_str("<ol class=\"list-decimal list-outside ml-8 pl-2 mb-4\">")
        }
        BlockType::BulletList => {
            html.push_str("<ul class=\"list-disc list-outside ml-8 pl-2 mb-4\">")
        }
        BlockType::RowItem => html.push_str("<div class=\"flex flex-col md:flex-row gap-4 my-4\">"),
        BlockType::ColumnItem => html.push_str("<div class=\"flex flex-col gap-4 my-4\">"),
        _ => {}
    }
}

fn close_list(html: &mut String, block_type: BlockType) {
    match block_type {
        BlockType::OrderedList => html.push_str("</ol>"),
        BlockType::BulletList => html.push_str("</ul>"),
        BlockType::RowItem => html.push_str("</div>"),
        BlockType::ColumnItem => html.push_str("</div>"),
        _ => {}
    }
}

fn open_block_tag(block_type: BlockType) -> Option<String> {
    match block_type {
        BlockType::None => None,
        BlockType::Paragraph => Some("<p>".to_string()),
        BlockType::Header(level) => Some(format!("<h{}>", level)),
        BlockType::Blockquote => {
            Some("<blockquote class=\"border-l-4 border-[#0071b8] pl-4 mb-4\">".to_string())
        }
        BlockType::CodeBlock => Some("<pre><code>".to_string()),
        BlockType::OrderedList
        | BlockType::BulletList
        | BlockType::RowItem
        | BlockType::ColumnItem => None,
    }
}

fn close_block_tag(block_type: BlockType) -> Option<String> {
    match block_type {
        BlockType::None => None,
        BlockType::Paragraph => Some("</p>".to_string()),
        BlockType::Header(level) => Some(format!("</h{}>", level)),
        BlockType::Blockquote => Some("</blockquote>".to_string()),
        BlockType::CodeBlock => Some("</code></pre>".to_string()),
        BlockType::OrderedList
        | BlockType::BulletList
        | BlockType::RowItem
        | BlockType::ColumnItem => None,
    }
}

/// How a button embed is drawn: centred on a line of its own, in the coral of
/// the site's sign-up button.
///
/// Styled inline, and not with the classes the rest of this output uses,
/// because the place a button most often goes is an email, and a mail client
/// honours nothing else. These are the styles of the button in the
/// confirmation email orrery sends.
const BUTTON_BLOCK_STYLE: &str = "text-align:center;margin:28px 0;";
const BUTTON_LINK_STYLE: &str = "display:inline-block;background-color:#D63511;color:#ffffff;font-weight:bold;text-decoration:none;padding:14px 32px;border-radius:6px;";

fn button_html(url: &str, label: &str) -> String {
    format!(
        "<p style=\"{}\"><a href=\"{}\" style=\"{}\">{}</a></p>",
        BUTTON_BLOCK_STYLE,
        html_escape(url),
        BUTTON_LINK_STYLE,
        html_escape(label)
    )
}

/// Whether an inline style draws a link as a button: a block of its own with
/// a filled background. True of the style above, and of the buttons written
/// by hand into the emails imported from orrery, which is what lets either
/// come back as a button embed.
fn is_button_style(style: &str) -> bool {
    let style: String = style
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();

    style.contains("display:inline-block") && style.contains("background-color:")
}

/// The button a paragraph consists of, as a button embed's value, when the
/// paragraph holds a button link and nothing else.
fn paragraph_button<'a>(node: &ego_tree::NodeRef<'a, scraper::Node>) -> Option<serde_json::Value> {
    use scraper::Node;

    let mut button = None;

    for child in node.children() {
        match child.value() {
            Node::Text(text) if text.text.trim().is_empty() => {}
            Node::Element(element)
                if button.is_none()
                    && element.name() == "a"
                    && element.attr("style").is_some_and(is_button_style) =>
            {
                let url = element.attr("href")?;
                let label: String = child
                    .descendants()
                    .filter_map(|n| match n.value() {
                        Node::Text(text) => Some(text.text.to_string()),
                        _ => None,
                    })
                    .collect();

                button = Some(serde_json::json!({ "url": url, "label": label.trim() }));
            }
            _ => return None,
        }
    }

    button
}

fn get_figure_class(align: &str) -> String {
    match align {
        "left" => "text-left mb-4".to_string(),
        "right" => "text-right mb-4".to_string(),
        "float-left" => "float-left mr-4 mb-4".to_string(),
        "float-right" => "float-right ml-4 mb-4".to_string(),
        _ => "text-center mx-auto mb-4".to_string(), // center is default
    }
}

fn get_block_type_from_attrs(attrs: &HashMap<String, serde_json::Value>) -> BlockType {
    if let Some(list_value) = attrs.get("list") {
        match list_value.as_str() {
            Some("ordered") => BlockType::OrderedList,
            _ => BlockType::BulletList,
        }
    } else if let Some(header) = attrs.get("header") {
        if let Some(level) = header.as_u64() {
            BlockType::Header(level)
        } else {
            BlockType::Paragraph
        }
    } else if attrs.contains_key("blockquote") {
        BlockType::Blockquote
    } else if attrs.contains_key("code-block") {
        BlockType::CodeBlock
    } else if attrs.contains_key("row-item") {
        BlockType::RowItem
    } else if attrs.contains_key("column-item") {
        BlockType::ColumnItem
    } else {
        BlockType::Paragraph
    }
}

fn apply_inline_formatting(text: &str, attrs: &HashMap<String, serde_json::Value>) -> String {
    let mut result = html_escape(text);

    // Apply inline styles in order
    if attrs.contains_key("bold") {
        result = format!("<strong>{}</strong>", result);
    }
    if attrs.contains_key("italic") {
        result = format!("<em>{}</em>", result);
    }
    if attrs.contains_key("underline") {
        result = format!("<u>{}</u>", result);
    }
    if attrs.contains_key("strike") {
        result = format!("<s>{}</s>", result);
    }
    if attrs.contains_key("code") {
        result = format!("<code>{}</code>", result);
    }

    // Handle script (superscript/subscript)
    if let Some(script) = attrs.get("script") {
        if let Some(script_val) = script.as_str() {
            match script_val {
                "super" => result = format!("<sup>{}</sup>", result),
                "sub" => result = format!("<sub>{}</sub>", result),
                _ => {}
            }
        }
    }

    // Handle links
    if let Some(link) = attrs.get("link") {
        if let Some(url) = link.as_str().and_then(sanitize_url) {
            result = format!("<a href=\"{}\">{}</a>", html_escape(url), result);
        }
    }

    // Handle color
    if let Some(color) = attrs.get("color") {
        if let Some(color_val) = color.as_str().and_then(sanitize_css_color) {
            result = format!("<span style=\"color: {}\">{}</span>", color_val, result);
        }
    }

    // Handle background
    if let Some(background) = attrs.get("background") {
        if let Some(bg_val) = background.as_str().and_then(sanitize_css_color) {
            result = format!(
                "<span style=\"background-color: {}\">{}</span>",
                bg_val, result
            );
        }
    }

    result
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Return a URL only if its scheme is safe to place in an `href`/`src`
/// attribute, otherwise `None`.
///
/// Rich-text values arrive as raw Delta JSON, so a malicious author can supply
/// a `javascript:` (or other active) scheme even though the legitimate editor
/// never emits one. We allow relative URLs (no scheme) and an explicit
/// allowlist of inert schemes; everything else is dropped.
fn sanitize_url(url: &str) -> Option<&str> {
    let trimmed = url.trim();

    // A scheme is `scheme:` before any '/', '?', or '#'. If there is no scheme
    // delimiter before those, the URL is relative and therefore safe.
    let scheme_end = trimmed.find(|c| c == ':' || c == '/' || c == '?' || c == '#');
    match scheme_end {
        Some(idx) if trimmed.as_bytes()[idx] == b':' => {
            let scheme = trimmed[..idx].to_ascii_lowercase();
            const ALLOWED: [&str; 4] = ["http", "https", "mailto", "tel"];
            if ALLOWED.contains(&scheme.as_str()) {
                Some(trimmed)
            } else {
                None
            }
        }
        // No ':' before the first path/query/fragment separator: relative URL.
        _ => Some(trimmed),
    }
}

/// Return the CSS color value only if it is a safe literal (hex color or a
/// plain alphanumeric color keyword), otherwise `None`.
///
/// This is interpolated into a `style` attribute, so anything outside this
/// strict shape (quotes, `url(...)`, `expression(...)`, semicolons) is dropped
/// to prevent attribute breakout and CSS-based injection.
fn sanitize_css_color(color: &str) -> Option<&str> {
    let trimmed = color.trim();
    let is_hex = trimmed.starts_with('#')
        && trimmed.len() > 1
        && trimmed[1..].chars().all(|c| c.is_ascii_hexdigit());
    let is_keyword = !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_alphabetic());
    if is_hex || is_keyword {
        Some(trimmed)
    } else {
        None
    }
}

/// Content format detected from input
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentFormat {
    /// Already in Delta JSON format
    Delta,
    /// HTML content
    Html,
    /// Markdown content
    Markdown,
    /// Empty content
    Empty,
}

/// Detect the format of content
///
/// Returns the detected format:
/// - `ContentFormat::Empty` if the content is empty or only whitespace
/// - `ContentFormat::Delta` if it starts with '{'
/// - `ContentFormat::Html` if it contains HTML tags
/// - `ContentFormat::Markdown` otherwise
pub fn detect_format(content: &str) -> ContentFormat {
    let trimmed = content.trim();

    if trimmed.is_empty() {
        return ContentFormat::Empty;
    }

    // If it starts with '{', assume it's Delta JSON
    if trimmed.starts_with('{') {
        return ContentFormat::Delta;
    }

    // Check if content contains HTML tags
    if trimmed.contains('<') && (trimmed.contains("</") || trimmed.contains("/>")) {
        return ContentFormat::Html;
    }

    // Default to Markdown
    ContentFormat::Markdown
}

/// Render content of any supported format (Delta JSON, HTML, or Markdown) to HTML.
///
/// Returns `(html, normalized_delta)` where `normalized_delta` is `Some` when
/// the input was converted from a non-Delta format. The caller should persist
/// the normalized Delta back to the database so future renders skip conversion
/// and the rich text editor can work with it directly.
pub fn content_to_html(content: &str) -> Result<(String, Option<String>), anyhow::Error> {
    let trimmed = content.trim();
    let format = detect_format(trimmed);

    match format {
        ContentFormat::Empty => Ok((String::new(), None)),
        ContentFormat::Delta => {
            let html = delta_to_html(trimmed)?;
            Ok((html, None))
        }
        ContentFormat::Html => {
            let delta_json = html_to_delta(trimmed)?;
            let html = delta_to_html(&delta_json)?;
            Ok((html, Some(delta_json)))
        }
        ContentFormat::Markdown => {
            let delta_json = markdown_to_delta(trimmed)?;
            let html = delta_to_html(&delta_json)?;
            Ok((html, Some(delta_json)))
        }
    }
}

/// Convert legacy content (HTML, Markdown, or existing Delta JSON) to Delta format
///
/// This function detects the format of the input and converts it to Quill Delta:
/// - If the input starts with '{', assumes it's already Delta JSON and returns it as-is
/// - If the input contains HTML tags, converts HTML to Delta
/// - Otherwise, treats it as Markdown and converts to Delta
///
/// You can optionally specify the format to skip auto-detection:
/// ```
/// use common::quill::{to_delta, ContentFormat};
///
/// // Auto-detect format
/// let result = to_delta("**bold**", None).unwrap();
///
/// // Force interpretation as Markdown
/// let result = to_delta("**bold**", Some(ContentFormat::Markdown)).unwrap();
/// ```
pub fn to_delta(
    content: &str,
    format: impl Into<Option<ContentFormat>>,
) -> Result<String, anyhow::Error> {
    let trimmed = content.trim();
    let format = format.into().unwrap_or_else(|| detect_format(trimmed));

    match format {
        ContentFormat::Empty => Ok(r#"{"ops":[{"insert":"\n"}]}"#.to_string()),
        ContentFormat::Delta => Ok(trimmed.to_string()),
        ContentFormat::Html => html_to_delta(trimmed),
        ContentFormat::Markdown => markdown_to_delta(trimmed),
    }
}

/// Convert HTML to Delta format
pub fn html_to_delta(html: &str) -> Result<String, anyhow::Error> {
    let document = Html::parse_fragment(html);
    let mut ops = Vec::new();

    // Process the root's children (not descendants, as we recursively walk in process_node)
    for node in document.root_element().children() {
        process_node(&node, &mut ops, &HashMap::new());
    }

    // Ensure we have at least one op
    if ops.is_empty() {
        ops.push(Op {
            insert: Some(Insert::Text("\n".to_string())),
            attributes: None,
        });
    } else {
        // Ensure ends with newline
        if let Some(last_op) = ops.last() {
            if let Some(Insert::Text(text)) = &last_op.insert {
                if !text.ends_with('\n') {
                    ops.push(Op {
                        insert: Some(Insert::Text("\n".to_string())),
                        attributes: None,
                    });
                }
            }
        }
    }

    let delta = Delta { ops };
    Ok(serde_json::to_string(&delta)?)
}

fn process_node<'a>(
    node: &ego_tree::NodeRef<'a, scraper::Node>,
    ops: &mut Vec<Op>,
    parent_attrs: &HashMap<String, serde_json::Value>,
) {
    use scraper::Node;

    match node.value() {
        Node::Text(text) => {
            let content = text.text.to_string();
            let trimmed = content.trim();
            // Only add text if it's not just whitespace
            if !trimmed.is_empty() {
                // Add text, and if there was space before/after in original, preserve one space
                let prefix = if content.starts_with(char::is_whitespace) && !ops.is_empty() {
                    " "
                } else {
                    ""
                };
                let suffix = if content.ends_with(char::is_whitespace) {
                    " "
                } else {
                    ""
                };

                let text_to_insert = format!("{}{}{}", prefix, trimmed, suffix);
                ops.push(Op {
                    insert: Some(Insert::Text(text_to_insert)),
                    attributes: if parent_attrs.is_empty() {
                        None
                    } else {
                        Some(parent_attrs.clone())
                    },
                });
            }
        }
        Node::Element(element) => {
            let mut attrs = parent_attrs.clone();
            let mut is_block = false;
            let mut block_attrs: Option<HashMap<String, serde_json::Value>> = None;

            match element.name() {
                // Block elements
                "p" => {
                    // A button is a line to itself, so the paragraph it was
                    // written in becomes the embed, with no line of its own.
                    if let Some(button) = paragraph_button(node) {
                        let mut embed = HashMap::new();
                        embed.insert("button".to_string(), button);
                        ops.push(Op {
                            insert: Some(Insert::Embed(embed)),
                            attributes: None,
                        });
                        return;
                    }

                    is_block = true;
                }
                "h1" => {
                    is_block = true;
                    block_attrs = Some({
                        let mut m = HashMap::new();
                        m.insert("header".to_string(), serde_json::json!(1));
                        m
                    });
                }
                "h2" => {
                    is_block = true;
                    block_attrs = Some({
                        let mut m = HashMap::new();
                        m.insert("header".to_string(), serde_json::json!(2));
                        m
                    });
                }
                "h3" => {
                    is_block = true;
                    block_attrs = Some({
                        let mut m = HashMap::new();
                        m.insert("header".to_string(), serde_json::json!(3));
                        m
                    });
                }
                "h4" => {
                    is_block = true;
                    block_attrs = Some({
                        let mut m = HashMap::new();
                        m.insert("header".to_string(), serde_json::json!(4));
                        m
                    });
                }
                "h5" => {
                    is_block = true;
                    block_attrs = Some({
                        let mut m = HashMap::new();
                        m.insert("header".to_string(), serde_json::json!(5));
                        m
                    });
                }
                "h6" => {
                    is_block = true;
                    block_attrs = Some({
                        let mut m = HashMap::new();
                        m.insert("header".to_string(), serde_json::json!(6));
                        m
                    });
                }
                "blockquote" => {
                    is_block = true;
                    block_attrs = Some({
                        let mut m = HashMap::new();
                        m.insert("blockquote".to_string(), serde_json::json!(true));
                        m
                    });
                }
                "li" => {
                    is_block = true;
                    // Determine if ordered or unordered by checking parent
                    let is_ordered = node.parent().map_or(false, |p| {
                        if let Node::Element(e) = p.value() {
                            e.name() == "ol"
                        } else {
                            false
                        }
                    });
                    block_attrs = Some({
                        let mut m = HashMap::new();
                        m.insert(
                            "list".to_string(),
                            serde_json::json!(if is_ordered { "ordered" } else { "bullet" }),
                        );
                        m
                    });
                }
                "br" => {
                    ops.push(Op {
                        insert: Some(Insert::Text("\n".to_string())),
                        attributes: None,
                    });
                    return;
                }
                "img" => {
                    if let Some(src) = element.attr("src") {
                        let mut embed = HashMap::new();
                        embed.insert("image".to_string(), serde_json::json!(src));
                        ops.push(Op {
                            insert: Some(Insert::Embed(embed)),
                            attributes: None,
                        });
                    }
                    return;
                }
                // Inline formatting
                "strong" | "b" => {
                    attrs.insert("bold".to_string(), serde_json::json!(true));
                }
                "em" | "i" => {
                    attrs.insert("italic".to_string(), serde_json::json!(true));
                }
                "u" => {
                    attrs.insert("underline".to_string(), serde_json::json!(true));
                }
                "s" | "strike" | "del" => {
                    attrs.insert("strike".to_string(), serde_json::json!(true));
                }
                "code" => {
                    attrs.insert("code".to_string(), serde_json::json!(true));
                }
                "a" => {
                    if let Some(href) = element.attr("href") {
                        attrs.insert("link".to_string(), serde_json::json!(href));
                    }
                }
                "sup" => {
                    attrs.insert("script".to_string(), serde_json::json!("super"));
                }
                "sub" => {
                    attrs.insert("script".to_string(), serde_json::json!("sub"));
                }
                _ => {}
            }

            // Process children
            for child in node.children() {
                process_node(&child, ops, &attrs);
            }

            // Add block terminator
            if is_block {
                ops.push(Op {
                    insert: Some(Insert::Text("\n".to_string())),
                    attributes: block_attrs,
                });
            }
        }
        _ => {
            // Process other node types' children
            for child in node.children() {
                process_node(&child, ops, parent_attrs);
            }
        }
    }
}

/// Convert Markdown to Delta format
fn markdown_to_delta(markdown: &str) -> Result<String, anyhow::Error> {
    let parser = Parser::new(markdown);
    let mut ops = Vec::new();
    let mut current_attrs = HashMap::new();
    let mut list_depth = 0;
    let mut current_list_type: Option<String> = None;

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { level, .. } => {
                    current_attrs.insert("header".to_string(), serde_json::json!(level as u64));
                }
                Tag::BlockQuote(_) => {
                    current_attrs.insert("blockquote".to_string(), serde_json::json!(true));
                }
                Tag::CodeBlock(_) => {
                    current_attrs.insert("code-block".to_string(), serde_json::json!(true));
                }
                Tag::List(first_item) => {
                    list_depth += 1;
                    current_list_type = if first_item.is_some() {
                        Some("ordered".to_string())
                    } else {
                        Some("bullet".to_string())
                    };
                }
                Tag::Item => {}
                Tag::Emphasis => {
                    current_attrs.insert("italic".to_string(), serde_json::json!(true));
                }
                Tag::Strong => {
                    current_attrs.insert("bold".to_string(), serde_json::json!(true));
                }
                Tag::Strikethrough => {
                    current_attrs.insert("strike".to_string(), serde_json::json!(true));
                }
                Tag::Link { dest_url, .. } => {
                    current_attrs
                        .insert("link".to_string(), serde_json::json!(dest_url.to_string()));
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Paragraph => {
                    ops.push(Op {
                        insert: Some(Insert::Text("\n".to_string())),
                        attributes: None,
                    });
                }
                TagEnd::Heading(_) => {
                    ops.push(Op {
                        insert: Some(Insert::Text("\n".to_string())),
                        attributes: Some(current_attrs.clone()),
                    });
                    current_attrs.clear();
                }
                TagEnd::BlockQuote(_) => {
                    ops.push(Op {
                        insert: Some(Insert::Text("\n".to_string())),
                        attributes: Some(current_attrs.clone()),
                    });
                    current_attrs.remove("blockquote");
                }
                TagEnd::CodeBlock => {
                    ops.push(Op {
                        insert: Some(Insert::Text("\n".to_string())),
                        attributes: Some(current_attrs.clone()),
                    });
                    current_attrs.remove("code-block");
                }
                TagEnd::List(_) => {
                    list_depth -= 1;
                    if list_depth == 0 {
                        current_list_type = None;
                    }
                }
                TagEnd::Item => {
                    if let Some(ref list_type) = current_list_type {
                        let mut list_attrs = HashMap::new();
                        list_attrs.insert("list".to_string(), serde_json::json!(list_type));
                        ops.push(Op {
                            insert: Some(Insert::Text("\n".to_string())),
                            attributes: Some(list_attrs),
                        });
                    }
                }
                TagEnd::Emphasis => {
                    current_attrs.remove("italic");
                }
                TagEnd::Strong => {
                    current_attrs.remove("bold");
                }
                TagEnd::Strikethrough => {
                    current_attrs.remove("strike");
                }
                TagEnd::Link => {
                    current_attrs.remove("link");
                }
                _ => {}
            },
            Event::Text(text) => {
                ops.push(Op {
                    insert: Some(Insert::Text(text.to_string())),
                    attributes: if current_attrs.is_empty() {
                        None
                    } else {
                        Some(current_attrs.clone())
                    },
                });
            }
            Event::Code(code) => {
                let mut code_attrs = HashMap::new();
                code_attrs.insert("code".to_string(), serde_json::json!(true));
                ops.push(Op {
                    insert: Some(Insert::Text(code.to_string())),
                    attributes: Some(code_attrs),
                });
            }
            Event::SoftBreak => {
                ops.push(Op {
                    insert: Some(Insert::Text(" ".to_string())),
                    attributes: None,
                });
            }
            Event::HardBreak => {
                ops.push(Op {
                    insert: Some(Insert::Text("\n".to_string())),
                    attributes: None,
                });
            }
            _ => {}
        }
    }

    // Ensure we end with a newline
    if let Some(last_op) = ops.last() {
        if let Some(Insert::Text(text)) = &last_op.insert {
            if !text.ends_with('\n') {
                ops.push(Op {
                    insert: Some(Insert::Text("\n".to_string())),
                    attributes: None,
                });
            }
        }
    } else {
        ops.push(Op {
            insert: Some(Insert::Text("\n".to_string())),
            attributes: None,
        });
    }

    let delta = Delta { ops };
    Ok(serde_json::to_string(&delta)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_text() {
        let delta = r#"{"ops":[{"insert":"Hello world\n"}]}"#;
        let html = delta_to_html(delta).unwrap();
        assert_eq!(html, r#"<p>Hello world</p>"#);
    }

    #[test]
    fn test_bold_text() {
        let delta = r#"{"ops":[{"insert":"Bold","attributes":{"bold":true}},{"insert":"\n"}]}"#;
        let html = delta_to_html(delta).unwrap();
        assert_eq!(html, r#"<p><strong>Bold</strong></p>"#);
    }

    #[test]
    fn test_heading() {
        let delta = r#"{"ops":[{"insert":"Heading"},{"attributes":{"header":1},"insert":"\n"}]}"#;
        let html = delta_to_html(delta).unwrap();
        assert_eq!(html, r#"<h1>Heading</h1>"#);
    }

    /// The markup of the button in the confirmation email orrery sends, which
    /// a button embed is meant to reproduce exactly.
    const ORRERY_BUTTON: &str = r#"<p style="text-align:center;margin:28px 0;"><a href="https://scistarter.org/verify?code={{VERIFY_CODE}}" style="display:inline-block;background-color:#D63511;color:#ffffff;font-weight:bold;text-decoration:none;padding:14px 32px;border-radius:6px;">CONFIRM MY EMAIL</a></p>"#;

    #[test]
    fn a_button_embed_is_drawn_like_the_button_in_orrerys_email() {
        let delta = r#"{"ops":[{"insert":"Before\n"},{"insert":{"button":{"url":"https://scistarter.org/verify?code={{VERIFY_CODE}}","label":"CONFIRM MY EMAIL"}}},{"insert":"After\n"}]}"#;

        assert_eq!(
            delta_to_html(delta).unwrap(),
            format!("<p>Before</p>{ORRERY_BUTTON}<p>After</p>")
        );
    }

    /// A merge field stands in for the whole link in an email, and has to
    /// survive to be merged.
    #[test]
    fn a_button_may_lead_to_a_merge_field() {
        let delta = r#"{"ops":[{"insert":{"button":{"url":"{{VERIFY_URL}}","label":"Go"}}}]}"#;

        assert!(
            delta_to_html(delta)
                .unwrap()
                .contains(r#"<a href="{{VERIFY_URL}}""#)
        );
    }

    #[test]
    fn a_button_is_escaped_and_cannot_carry_a_script() {
        let delta = r#"{"ops":[{"insert":{"button":{"url":"https://example.org/?a=1&b=\"2\"","label":"<b>Go</b>"}}}]}"#;
        let html = delta_to_html(delta).unwrap();

        assert!(html.contains(r#"href="https://example.org/?a=1&amp;b=&quot;2&quot;""#));
        assert!(html.contains("&lt;b&gt;Go&lt;/b&gt;"));

        for delta in [
            r#"{"ops":[{"insert":{"button":{"url":"javascript:alert(1)","label":"Go"}}}]}"#,
            r#"{"ops":[{"insert":{"button":{"url":"https://example.org/","label":"  "}}}]}"#,
            r#"{"ops":[{"insert":{"button":{"label":"Go"}}}]}"#,
        ] {
            assert_eq!(delta_to_html(delta).unwrap(), "", "{delta}");
        }
    }

    /// An email imported from orrery keeps the button somebody wrote into it
    /// by hand, and one this module drew comes back as it went out.
    #[test]
    fn a_paragraph_that_is_a_button_is_imported_as_one() {
        let html = format!("<p>Before</p>\n\n{ORRERY_BUTTON}\n\n<p>After</p>");
        let delta = html_to_delta(&html).unwrap();

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&delta).unwrap(),
            serde_json::json!({"ops": [
                {"insert": "Before"},
                {"insert": "\n"},
                {"insert": {"button": {
                    "url": "https://scistarter.org/verify?code={{VERIFY_CODE}}",
                    "label": "CONFIRM MY EMAIL"
                }}},
                {"insert": "After"},
                {"insert": "\n"},
            ]})
        );

        assert_eq!(
            delta_to_html(&delta).unwrap(),
            format!("<p>Before</p>{ORRERY_BUTTON}<p>After</p>")
        );
    }

    /// Only a paragraph given over to the button becomes one. A styled link
    /// in the middle of a sentence has words either side of it that a block
    /// of its own would strand.
    #[test]
    fn a_styled_link_inside_a_sentence_stays_a_link() {
        let html = r#"<p>Please <a href="https://example.org/" style="display:inline-block;background-color:#D63511;">confirm</a> today.</p>"#;
        let delta = html_to_delta(html).unwrap();

        assert!(!delta.contains("button"), "{delta}");
        assert!(
            delta.contains(r#""link":"https://example.org/""#),
            "{delta}"
        );
    }

    #[test]
    fn an_ordinary_link_alone_in_a_paragraph_stays_a_link() {
        let html = r#"<p><a href="https://example.org/">Read more</a></p>"#;
        let delta = html_to_delta(html).unwrap();

        assert!(!delta.contains("button"), "{delta}");
    }

    #[test]
    fn test_figure_with_caption() {
        let delta = r#"{"ops":[{"insert":"Here is an image with a caption:\n"},{"insert":{"figure":{"url":"https://example.com/image.jpg","caption":"A beautiful sunset","align":"center"}}},{"insert":"\n"}]}"#;
        let html = delta_to_html(delta).unwrap();
        assert!(html.contains("<figure class=\"text-center mx-auto mb-4\">"));
        assert!(
            html.contains(
                "<img src=\"https://example.com/image.jpg\" alt=\"A beautiful sunset\" />"
            )
        );
        assert!(html.contains("<figcaption>A beautiful sunset</figcaption>"));
    }

    #[test]
    fn test_figure_with_width() {
        let delta = r#"{"ops":[{"insert":{"figure":{"url":"https://example.com/test.png","caption":"Test image","align":"center","width":"50%"}}},{"insert":"\n"}]}"#;
        let html = delta_to_html(delta).unwrap();
        assert!(html.contains("<figure class=\"text-center mx-auto mb-4\">"));
        assert!(html.contains(
            "<img src=\"https://example.com/test.png\" alt=\"Test image\" style=\"width: 50%\" />"
        ));
        assert!(html.contains("<figcaption>Test image</figcaption>"));
    }

    #[test]
    fn test_figure_float_left() {
        let delta = r#"{"ops":[{"insert":{"figure":{"url":"https://example.com/test.png","caption":"Test image","align":"float-left"}}},{"insert":"Some text that wraps around.\n"}]}"#;
        let html = delta_to_html(delta).unwrap();
        assert!(html.contains("<figure class=\"float-left mr-4 mb-4\">"));
    }

    #[test]
    fn test_complex_formatting() {
        let delta = r##"{"ops":[{"insert":"A "},{"attributes":{"italic":true},"insert":"silly"},{"insert":" story"},{"attributes":{"header":1},"insert":"\n"},{"attributes":{"italic":true,"bold":true},"insert":"Bosworth"},{"attributes":{"italic":true},"insert":" Fenton"},{"insert":" was a "},{"attributes":{"strike":true},"insert":"real"},{"insert":" "},{"attributes":{"color":"#9933ff"},"insert":"mean "},{"attributes":{"color":"#9933ff","italic":true},"insert":"du"},{"attributes":{"italic":true},"insert":"de"},{"insert":". He was flarb, and glip, and totally shnub. And then there was all the flulloboling.\nSo, then one day I was lounging on the front steps, enjoying the spring weather, when up walks Bosworth with an unfortunate grin on his face.\nOl' Bosworth, he says\nYo, my dud"},{"attributes":{"script":"super"},"insert":"1"},{"insert":"? How goes the wasting of your life?"},{"attributes":{"blockquote":true},"insert":"\n"},{"insert":"To which I responded\nMy life rocks."},{"attributes":{"list":"ordered"},"insert":"\n"},{"insert":"When was the last time "},{"attributes":{"italic":true},"insert":"you"},{"insert":" did something useful?"},{"attributes":{"list":"ordered"},"insert":"\n"},{"insert":"Go away."},{"attributes":{"list":"ordered"},"insert":"\n"},{"insert":"\n\n\n1. Yes, he said \"dud\"\n"}]}"##;
        let html = delta_to_html(delta).unwrap();
        let expected = r#"<h1>A <em>silly</em> story</h1><p><em><strong>Bosworth</strong></em><em> Fenton</em> was a <s>real</s> <span style="color: #9933ff">mean </span><span style="color: #9933ff"><em>du</em></span><em>de</em>. He was flarb, and glip, and totally shnub. And then there was all the flulloboling.</p><p>So, then one day I was lounging on the front steps, enjoying the spring weather, when up walks Bosworth with an unfortunate grin on his face.</p><p>Ol&#39; Bosworth, he says</p><blockquote class="border-l-4 border-[#0071b8] pl-4 mb-4">Yo, my dud<sup>1</sup>? How goes the wasting of your life?</blockquote><p>To which I responded</p><ol class="list-decimal list-outside ml-8 pl-2 mb-4"><li>My life rocks.</li><li>When was the last time <em>you</em> did something useful?</li><li>Go away.</li></ol><p><br></p><p><br></p><p><br></p><p>1. Yes, he said &quot;dud&quot;</p>"#;
        assert_eq!(html, expected);
    }

    #[test]
    fn test_to_delta_with_existing_delta() {
        let delta = r#"{"ops":[{"insert":"Hello world\n"}]}"#;
        let result = to_delta(delta, None).unwrap();
        let parsed: Delta = serde_json::from_str(&result).unwrap();

        // Should have exactly one op
        assert_eq!(parsed.ops.len(), 1);

        // Should contain the text "Hello world\n"
        if let Some(Insert::Text(text)) = &parsed.ops[0].insert {
            assert_eq!(text, "Hello world\n");
        } else {
            panic!("Expected text insert");
        }

        // Should have no attributes
        assert!(parsed.ops[0].attributes.is_none());
    }

    #[test]
    fn test_to_delta_with_html() {
        let html = "<p>Hello <strong>world</strong></p>";
        let result = to_delta(html, None).unwrap();
        let parsed: Delta = serde_json::from_str(&result).unwrap();

        // Should have 3 ops: "Hello ", "world" (bold), "\n"
        assert_eq!(parsed.ops.len(), 3);

        // First op: "Hello " with no attributes
        if let Some(Insert::Text(text)) = &parsed.ops[0].insert {
            assert_eq!(text, "Hello ");
            assert!(parsed.ops[0].attributes.is_none());
        } else {
            panic!("Expected text insert for first op");
        }

        // Second op: "world" with bold attribute
        if let Some(Insert::Text(text)) = &parsed.ops[1].insert {
            assert_eq!(text, "world");
            let attrs = parsed.ops[1]
                .attributes
                .as_ref()
                .expect("Expected attributes");
            assert_eq!(attrs.get("bold").unwrap().as_bool().unwrap(), true);
        } else {
            panic!("Expected text insert for second op");
        }

        // Third op: "\n" (paragraph end)
        if let Some(Insert::Text(text)) = &parsed.ops[2].insert {
            assert_eq!(text, "\n");
        } else {
            panic!("Expected text insert for third op");
        }
    }

    #[test]
    fn test_to_delta_with_markdown() {
        let markdown = "# Hello World\n\nThis is **bold** text.";
        let result = to_delta(markdown, None).unwrap();
        let parsed: Delta = serde_json::from_str(&result).unwrap();

        // Should have: "Hello World", "\n" (with header), "This is ", "bold" (with bold), " text.", "\n"
        assert!(parsed.ops.len() >= 5);

        // First op should be heading text
        if let Some(Insert::Text(text)) = &parsed.ops[0].insert {
            assert_eq!(text, "Hello World");
        } else {
            panic!("Expected text insert for heading");
        }

        // Second op should be newline with header attribute
        if let Some(Insert::Text(text)) = &parsed.ops[1].insert {
            assert_eq!(text, "\n");
            let attrs = parsed.ops[1]
                .attributes
                .as_ref()
                .expect("Expected header attribute");
            assert_eq!(attrs.get("header").unwrap().as_u64().unwrap(), 1);
        } else {
            panic!("Expected newline with header attribute");
        }
    }

    #[test]
    fn test_html_to_delta_simple() {
        let html = "<p>Simple paragraph</p>";
        let result = html_to_delta(html).unwrap();
        let parsed: Delta = serde_json::from_str(&result).unwrap();

        // Should have 2 ops: text + newline
        assert_eq!(parsed.ops.len(), 2);

        // First op: "Simple paragraph"
        if let Some(Insert::Text(text)) = &parsed.ops[0].insert {
            assert_eq!(text, "Simple paragraph");
        } else {
            panic!("Expected text insert");
        }

        // Second op: "\n"
        if let Some(Insert::Text(text)) = &parsed.ops[1].insert {
            assert_eq!(text, "\n");
        } else {
            panic!("Expected newline");
        }
    }

    #[test]
    fn test_html_to_delta_with_formatting() {
        let html = "<p>This is <strong>bold</strong> and <em>italic</em> text</p>";
        let result = html_to_delta(html).unwrap();
        let parsed: Delta = serde_json::from_str(&result).unwrap();

        // Should have: "This is ", "bold" (bold), " and ", "italic" (italic), " text", "\n"
        assert_eq!(parsed.ops.len(), 6);

        // Check the bold text
        if let Some(Insert::Text(text)) = &parsed.ops[1].insert {
            assert_eq!(text, "bold");
            let attrs = parsed.ops[1]
                .attributes
                .as_ref()
                .expect("Expected bold attribute");
            assert_eq!(attrs.get("bold").unwrap().as_bool().unwrap(), true);
        } else {
            panic!("Expected bold text");
        }

        // Check the italic text
        if let Some(Insert::Text(text)) = &parsed.ops[3].insert {
            assert_eq!(text, "italic");
            let attrs = parsed.ops[3]
                .attributes
                .as_ref()
                .expect("Expected italic attribute");
            assert_eq!(attrs.get("italic").unwrap().as_bool().unwrap(), true);
        } else {
            panic!("Expected italic text");
        }
    }

    #[test]
    fn test_html_to_delta_heading() {
        let html = "<h2>Second Level Heading</h2>";
        let result = html_to_delta(html).unwrap();
        let parsed: Delta = serde_json::from_str(&result).unwrap();

        // Should have 2 ops: text + newline with header attribute
        assert_eq!(parsed.ops.len(), 2);

        if let Some(Insert::Text(text)) = &parsed.ops[0].insert {
            assert_eq!(text, "Second Level Heading");
        } else {
            panic!("Expected heading text");
        }

        if let Some(Insert::Text(text)) = &parsed.ops[1].insert {
            assert_eq!(text, "\n");
            let attrs = parsed.ops[1]
                .attributes
                .as_ref()
                .expect("Expected header attribute");
            assert_eq!(attrs.get("header").unwrap().as_u64().unwrap(), 2);
        } else {
            panic!("Expected newline with header attribute");
        }
    }

    #[test]
    fn test_html_to_delta_list() {
        let html = "<ul><li>First item</li><li>Second item</li></ul>";
        let result = html_to_delta(html).unwrap();
        let parsed: Delta = serde_json::from_str(&result).unwrap();

        // Should have 4 ops: "First item", "\n" (list), "Second item", "\n" (list)
        assert_eq!(parsed.ops.len(), 4);

        // Check first list item
        if let Some(Insert::Text(text)) = &parsed.ops[0].insert {
            assert_eq!(text, "First item");
        } else {
            panic!("Expected first item text");
        }

        // Check first newline has list attribute
        if let Some(Insert::Text(text)) = &parsed.ops[1].insert {
            assert_eq!(text, "\n");
            let attrs = parsed.ops[1]
                .attributes
                .as_ref()
                .expect("Expected list attribute");
            assert_eq!(attrs.get("list").unwrap().as_str().unwrap(), "bullet");
        } else {
            panic!("Expected newline with list attribute");
        }

        // Check second list item
        if let Some(Insert::Text(text)) = &parsed.ops[2].insert {
            assert_eq!(text, "Second item");
        } else {
            panic!("Expected second item text");
        }
    }

    #[test]
    fn test_markdown_to_delta_heading() {
        let markdown = "# Heading 1";
        let result = markdown_to_delta(markdown).unwrap();
        let parsed: Delta = serde_json::from_str(&result).unwrap();

        // Should have 2 ops: text + newline with header attribute
        assert_eq!(parsed.ops.len(), 2);

        if let Some(Insert::Text(text)) = &parsed.ops[0].insert {
            assert_eq!(text, "Heading 1");
        } else {
            panic!("Expected heading text");
        }

        if let Some(Insert::Text(text)) = &parsed.ops[1].insert {
            assert_eq!(text, "\n");
            let attrs = parsed.ops[1]
                .attributes
                .as_ref()
                .expect("Expected header attribute");
            assert_eq!(attrs.get("header").unwrap().as_u64().unwrap(), 1);
        } else {
            panic!("Expected newline with header attribute");
        }
    }

    #[test]
    fn test_markdown_to_delta_list() {
        let markdown = "- Item 1\n- Item 2";
        let result = markdown_to_delta(markdown).unwrap();
        let parsed: Delta = serde_json::from_str(&result).unwrap();

        // Should have 4 ops: "Item 1", "\n" (list), "Item 2", "\n" (list)
        assert_eq!(parsed.ops.len(), 4);

        // Check first item
        if let Some(Insert::Text(text)) = &parsed.ops[0].insert {
            assert_eq!(text, "Item 1");
        } else {
            panic!("Expected first item text");
        }

        // Check first newline with list attribute
        if let Some(Insert::Text(text)) = &parsed.ops[1].insert {
            assert_eq!(text, "\n");
            let attrs = parsed.ops[1]
                .attributes
                .as_ref()
                .expect("Expected list attribute");
            assert_eq!(attrs.get("list").unwrap().as_str().unwrap(), "bullet");
        } else {
            panic!("Expected newline with list attribute");
        }
    }

    #[test]
    fn test_markdown_to_delta_bold_and_italic() {
        let markdown = "This is **bold** and *italic* text";
        let result = markdown_to_delta(markdown).unwrap();
        let parsed: Delta = serde_json::from_str(&result).unwrap();

        // Should have: "This is ", "bold" (bold), " and ", "italic" (italic), " text", "\n"
        assert_eq!(parsed.ops.len(), 6);

        // Check bold text
        if let Some(Insert::Text(text)) = &parsed.ops[1].insert {
            assert_eq!(text, "bold");
            let attrs = parsed.ops[1]
                .attributes
                .as_ref()
                .expect("Expected bold attribute");
            assert_eq!(attrs.get("bold").unwrap().as_bool().unwrap(), true);
        } else {
            panic!("Expected bold text");
        }

        // Check italic text
        if let Some(Insert::Text(text)) = &parsed.ops[3].insert {
            assert_eq!(text, "italic");
            let attrs = parsed.ops[3]
                .attributes
                .as_ref()
                .expect("Expected italic attribute");
            assert_eq!(attrs.get("italic").unwrap().as_bool().unwrap(), true);
        } else {
            panic!("Expected italic text");
        }
    }

    #[test]
    fn test_to_delta_empty_string() {
        let result = to_delta("", None).unwrap();
        let parsed: Delta = serde_json::from_str(&result).unwrap();

        // Should return a single newline
        assert_eq!(parsed.ops.len(), 1);
        if let Some(Insert::Text(text)) = &parsed.ops[0].insert {
            assert_eq!(text, "\n");
        } else {
            panic!("Expected newline for empty input");
        }
    }

    #[test]
    fn test_detect_format_empty() {
        assert_eq!(detect_format(""), ContentFormat::Empty);
        assert_eq!(detect_format("   "), ContentFormat::Empty);
        assert_eq!(detect_format("\n\n"), ContentFormat::Empty);
    }

    #[test]
    fn test_detect_format_delta() {
        assert_eq!(
            detect_format(r#"{"ops":[{"insert":"hello\n"}]}"#),
            ContentFormat::Delta
        );
        assert_eq!(detect_format("{\"test\": true}"), ContentFormat::Delta);
    }

    #[test]
    fn test_detect_format_html() {
        assert_eq!(detect_format("<p>Hello world</p>"), ContentFormat::Html);
        assert_eq!(
            detect_format("Text with <strong>bold</strong>"),
            ContentFormat::Html
        );
        assert_eq!(detect_format("<br/>"), ContentFormat::Html);
        assert_eq!(detect_format("Line<br />break"), ContentFormat::Html);
    }

    #[test]
    fn test_detect_format_markdown() {
        assert_eq!(detect_format("# Heading"), ContentFormat::Markdown);
        assert_eq!(
            detect_format("This is **bold** text"),
            ContentFormat::Markdown
        );
        assert_eq!(detect_format("Plain text"), ContentFormat::Markdown);
        // Less-than without closing tag is treated as markdown
        assert_eq!(detect_format("5 < 10"), ContentFormat::Markdown);
    }
}
