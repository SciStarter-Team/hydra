use sailfish::TemplateOnce;

/// Convert HTML to markdown for plain text email version
pub fn html_to_markdown(html: &str) -> String {
    html2md::parse_html(html)
}

/// One network in the footer's "Follow SciStarter" row.
struct SocialLink {
    name: &'static str,

    /// Stem of the network's icon in the frontend's `static/img/email/`. The
    /// icons are PNGs because email clients do not render SVG.
    icon: &'static str,

    url: &'static str,
}

const SOCIAL_LINKS: [SocialLink; 4] = [
    SocialLink {
        name: "Facebook",
        icon: "facebook",
        url: "https://www.facebook.com/SciStarter/",
    },
    SocialLink {
        name: "Instagram",
        icon: "instagram",
        url: "https://www.instagram.com/scistarter/",
    },
    SocialLink {
        name: "LinkedIn",
        icon: "linkedin",
        url: "https://www.linkedin.com/company/scistarter/",
    },
    SocialLink {
        name: "YouTube",
        icon: "youtube",
        url: "https://www.youtube.com/channel/UCrsleLmnQlaPm5BoYHRiKrQ",
    },
];

const ORGANIZATION: &str = "SciStarter Foundation";
const ORGANIZATION_LOCATION: &str = "Philadelphia, PA, USA";

/// The year the footer's copyright line carries.
fn copyright_year() -> i16 {
    jiff::Zoned::now().year()
}

#[derive(TemplateOnce)]
#[template(path = "layout.stpl.html")]
struct EmailLayout<'a> {
    content: &'a str,
    /// Where the footer's Unsubscribe link leads, or None for an email that
    /// offers no way to unsubscribe.
    unsubscribe_path: Option<&'a str>,
    open_path: &'a str,
    year: i16,
}

/// Wrap HTML content in standard email layout. The footer offers to
/// unsubscribe at `unsubscribe_path`, or not at all when that is None.
pub fn wrap_html_layout(
    body_html: &str,
    unsubscribe_path: Option<&str>,
    open_path: &str,
) -> String {
    EmailLayout {
        content: body_html,
        unsubscribe_path,
        open_path,
        year: copyright_year(),
    }
    .render_once()
    .expect("Failed to render email layout template")
}

/// The plain text rendition of the footer the HTML layout carries. As there,
/// an `unsubscribe_url` of None leaves out the offer to unsubscribe.
pub fn plain_text_footer(unsubscribe_url: Option<&str>) -> String {
    let follow: String = SOCIAL_LINKS
        .iter()
        .map(|link| format!("{}: {}\n", link.name, link.url))
        .collect();

    let unsubscribe = unsubscribe_url
        .map(|url| format!("To unsubscribe, click: {url}\n"))
        .unwrap_or_default();

    format!(
        r#"

---
Follow SciStarter:
{follow}
SciStarter.org: https://{domain}
Manage email preferences: https://{domain}/dashboard/settings
{unsubscribe}
{ORGANIZATION}
{ORGANIZATION_LOCATION}

© {year} {ORGANIZATION}
"#,
        domain = env!("DOMAIN"),
        year = copyright_year(),
    )
}

/// Append footer to plain text email
pub fn add_footer_to_plain_text(text: &str, unsubscribe_url: Option<&str>) -> String {
    format!("{}{}", text, plain_text_footer(unsubscribe_url))
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
        let wrapped = wrap_html_layout(body, Some(unsubscribe_path), open_path);
        assert!(wrapped.contains("<!DOCTYPE html>"));
        assert!(wrapped.contains("<p>Test content</p>"));
        // Verify the paths are present (HTML-escaped ampersands)
        assert!(wrapped.contains("/messaging/emailer/unsubscribe?a=123&amp;b=456"));
        assert!(wrapped.contains("/messaging/emailer/open?a=123&amp;b=456"));
    }

    #[test]
    fn test_layout_footer() {
        let wrapped = wrap_html_layout("<p>Test content</p>", Some("/unsubscribe"), "/open");
        for link in &SOCIAL_LINKS {
            assert!(wrapped.contains(link.url));
            assert!(wrapped.contains(&format!("/-static/img/email/{}.png", link.icon)));
        }
        assert!(wrapped.contains(&format!("&copy; {} {ORGANIZATION}", copyright_year())));
    }

    /// An email its recipient cannot opt out of offers no way to unsubscribe,
    /// and loses nothing else from its footer.
    #[test]
    fn test_footer_without_unsubscribe() {
        let wrapped = wrap_html_layout("<p>Test content</p>", None, "/open");
        assert!(!wrapped.contains("nsubscribe"));
        assert!(wrapped.contains("Manage email preferences"));
        assert!(wrapped.contains(ORGANIZATION_LOCATION));

        let text = add_footer_to_plain_text("Email body", None);
        assert!(!text.contains("nsubscribe"));
        assert!(text.contains("Manage email preferences"));
        assert!(text.contains(&format!("\n\n{ORGANIZATION}\n{ORGANIZATION_LOCATION}\n")));

        // With the offer, the same footer is spaced the same way.
        let text = add_footer_to_plain_text("Email body", Some("https://example.org/u"));
        assert!(text.contains(&format!(
            "To unsubscribe, click: https://example.org/u\n\n{ORGANIZATION}\n"
        )));
    }

    #[test]
    fn test_add_footer() {
        let text = "Email body";
        let unsubscribe_url = "https://example.org/messaging/emailer/unsubscribe?a=123&b=456";
        let with_footer = add_footer_to_plain_text(text, Some(unsubscribe_url));
        assert!(with_footer.starts_with("Email body"));
        assert!(with_footer.contains("SciStarter"));
        assert!(with_footer.contains(&format!("To unsubscribe, click: {unsubscribe_url}\n")));
    }
}
