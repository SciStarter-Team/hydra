//! Markdown written by site members, rendered for other people to read.

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

/// Render Markdown that a member wrote (an ambassador's bio, say) as HTML that
/// is safe to show to everyone else.
///
/// - Raw HTML is shown as the text it is, never interpreted.
/// - Links survive only to `http`, `https` and `mailto` destinations, and go
///   out as `rel="nofollow ugc noopener noreferrer"`; any other link is
///   unwrapped to its text, which rules out `javascript:` and `data:` URLs.
/// - Images are dropped, alt text included, so nobody's page loads pictures
///   from wherever a member pointed it.
pub fn user_content_html(markdown: &str) -> String {
    let mut events: Vec<Event> = Vec::new();
    // Per open link, whether its closing tag should be written.
    let mut link_kept: Vec<bool> = Vec::new();
    let mut image_depth = 0usize;

    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::Image { .. }) => image_depth += 1,
            Event::End(TagEnd::Image) => image_depth -= 1,
            _ if image_depth > 0 => {}
            Event::Start(Tag::Link { dest_url, .. }) => {
                let kept = allowed_destination(&dest_url);
                if kept {
                    events.push(Event::Html(
                        format!(
                            r#"<a href="{}" rel="nofollow ugc noopener noreferrer">"#,
                            escape_attribute(&dest_url)
                        )
                        .into(),
                    ));
                }
                link_kept.push(kept);
            }
            Event::End(TagEnd::Link) => {
                if link_kept.pop().unwrap_or(false) {
                    events.push(Event::Html("</a>".into()));
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => events.push(Event::Text(html)),
            other => events.push(other),
        }
    }

    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events.into_iter());
    html
}

fn allowed_destination(url: &str) -> bool {
    let url = url.trim_start().to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| url.starts_with(scheme))
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting_survives() {
        assert_eq!(
            user_content_html("I *love* **bugs**."),
            "<p>I <em>love</em> <strong>bugs</strong>.</p>\n"
        );
    }

    #[test]
    fn raw_html_is_shown_not_run() {
        let html = user_content_html("<script>alert(1)</script>\n\nHi <b onclick=x>there</b>");
        assert!(!html.contains("<script"));
        assert!(!html.contains("<b "));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn web_links_are_kept_and_marked_as_user_content() {
        assert_eq!(
            user_content_html("[my lab](https://example.org/?a=1&b=\"2\")"),
            "<p><a href=\"https://example.org/?a=1&amp;b=&quot;2&quot;\" \
             rel=\"nofollow ugc noopener noreferrer\">my lab</a></p>\n"
        );
    }

    #[test]
    fn script_links_are_unwrapped() {
        assert_eq!(
            user_content_html("[click](javascript:alert(1)) [x]( JavaScript:alert(1))"),
            "<p>click x</p>\n"
        );
    }

    #[test]
    fn images_are_dropped() {
        assert_eq!(
            user_content_html("before ![alt text](https://example.org/a.png) after"),
            "<p>before  after</p>\n"
        );
    }
}
