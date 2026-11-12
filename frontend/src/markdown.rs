//! Markdown written by site members, rendered for other people to read.

use pulldown_cmark::{CowStr, Event, Parser, Tag, TagEnd};

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
    render(markdown, Source::Member)
}

/// Render the Markdown of a hosted form (its introduction, its notes, the
/// help beside a question) as HTML that is safe to show to everyone. Forms
/// copied from orrery were written by whoever built them there, so it is
/// held to what [`user_content_html`] is, but for two things a form needs:
///
/// - Pictures are kept, where they are on the web (`https`) or on this site,
///   since a form shows what to look for.
/// - Links to this site's own pages (`/projects/...`) are kept too.
pub fn form_content_html(markdown: &str) -> String {
    render(markdown, Source::Form)
}

/// Who wrote the Markdown being rendered, which decides what of it is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Member,
    Form,
}

impl Source {
    fn keeps_link(self, url: &str) -> bool {
        allowed_destination(url) || (self == Source::Form && site_path(url))
    }

    fn keeps_image(self, url: &str) -> bool {
        self == Source::Form && (url.trim_start().starts_with("https://") || site_path(url))
    }
}

fn render(markdown: &str, source: Source) -> String {
    let mut events: Vec<Event> = Vec::new();
    // Per open link, whether its closing tag should be written.
    let mut link_kept: Vec<bool> = Vec::new();
    // Per open image, whether it is kept: its alt text is written into the
    // tag rather than shown, and a dropped image's is not written at all.
    let mut image_kept: Vec<bool> = Vec::new();

    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::Image { dest_url, .. }) => {
                let kept = source.keeps_image(&dest_url);
                if kept {
                    events.push(Event::Start(Tag::Image {
                        link_type: pulldown_cmark::LinkType::Inline,
                        dest_url,
                        title: CowStr::Borrowed(""),
                        id: CowStr::Borrowed(""),
                    }));
                }
                image_kept.push(kept);
            }
            Event::End(TagEnd::Image) => {
                if image_kept.pop().unwrap_or(false) {
                    events.push(Event::End(TagEnd::Image));
                }
            }
            Event::Text(text) if image_kept.last().is_some_and(|kept| *kept) => {
                events.push(Event::Text(text));
            }
            _ if !image_kept.is_empty() => {}
            Event::Start(Tag::Link { dest_url, .. }) => {
                let kept = source.keeps_link(&dest_url);
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

/// Whether `url` is a path on this site: it starts with one slash, not the
/// two that would name another host.
fn site_path(url: &str) -> bool {
    let url = url.trim_start();
    url.starts_with('/') && !url.starts_with("//") && !url.starts_with("/\\")
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
    fn a_form_keeps_pictures_from_the_web_and_from_this_site() {
        assert_eq!(
            form_content_html(
                "![a bee](https://example.org/bee.png) ![](/-static/img/forms/x.jpg)"
            ),
            "<p><img src=\"https://example.org/bee.png\" alt=\"a bee\" /> \
             <img src=\"/-static/img/forms/x.jpg\" alt=\"\" /></p>\n"
        );
    }

    #[test]
    fn a_form_drops_pictures_from_anywhere_else() {
        assert_eq!(
            form_content_html(
                "![a](http://example.org/a.png)![b](//example.org/b.png)![c](data:image/png;base64,AAAA)"
            ),
            "<p></p>\n"
        );
    }

    #[test]
    fn a_form_keeps_links_to_this_sites_pages_only() {
        assert_eq!(
            form_content_html("[squirrels](/projects/squirrels) [away](//evil.example)"),
            "<p><a href=\"/projects/squirrels\" rel=\"nofollow ugc noopener noreferrer\">squirrels</a> away</p>\n"
        );
        assert_eq!(
            user_content_html("[squirrels](/projects/squirrels)"),
            "<p>squirrels</p>\n"
        );
    }

    #[test]
    fn a_form_shows_raw_html_as_text() {
        let html = form_content_html("<img src=x onerror=alert(1)>");
        assert!(!html.contains("<img"));
    }

    #[test]
    fn images_are_dropped() {
        assert_eq!(
            user_content_html("before ![alt text](https://example.org/a.png) after"),
            "<p>before  after</p>\n"
        );
    }
}
