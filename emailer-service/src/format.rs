use sailfish::TemplateOnce;

/// Convert HTML to markdown for plain text email version
pub fn html_to_markdown(html: &str) -> String {
    html2md::parse_html(html)
}

#[derive(TemplateOnce)]
#[template(path = "layout.stpl.html")]
struct EmailLayout<'a> {
    content: &'a str,
    unsubscribe_path: &'a str,
    open_path: &'a str,
}

/// Wrap HTML content in standard email layout
pub fn wrap_html_layout(body_html: &str, unsubscribe_path: &str, open_path: &str) -> String {
    EmailLayout {
        content: body_html,
        unsubscribe_path,
        open_path,
    }
    .render_once()
    .expect("Failed to render email layout template")
}

/// Get standard plain text footer
pub fn get_plain_text_footer() -> &'static str {
    r#"

---
This email was sent by SciStarter
To unsubscribe, click: {{UNSUBSCRIBE_URL}}
"#
}

/// Append footer to plain text email
pub fn add_footer_to_plain_text(text: &str) -> String {
    format!("{}{}", text, get_plain_text_footer())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_to_markdown() {
        let html = "<h1>Hello</h1><p>This is <strong>bold</strong> text.</p>";
        let markdown = html_to_markdown(html);
        assert!(markdown.contains("Hello"));
        assert!(markdown.contains("**bold**") || markdown.contains("bold")); // Different versions may format differently
    }

    #[test]
    fn test_wrap_html_layout() {
        let body = "<p>Test content</p>";
        let unsubscribe_path = "/messaging/emailer/unsubscribe?a=123&b=456";
        let open_path = "/messaging/emailer/open?a=123&b=456";
        let wrapped = wrap_html_layout(body, unsubscribe_path, open_path);
        assert!(wrapped.contains("<!DOCTYPE html>"));
        assert!(wrapped.contains("<p>Test content</p>"));
        // Verify the paths are present (HTML-escaped ampersands)
        assert!(wrapped.contains("/messaging/emailer/unsubscribe?a=123&amp;b=456"));
        assert!(wrapped.contains("/messaging/emailer/open?a=123&amp;b=456"));
    }

    #[test]
    fn test_add_footer() {
        let text = "Email body";
        let with_footer = add_footer_to_plain_text(text);
        assert!(with_footer.starts_with("Email body"));
        assert!(with_footer.contains("SciStarter"));
        assert!(with_footer.contains("{{UNSUBSCRIBE_URL}}"));
    }
}
