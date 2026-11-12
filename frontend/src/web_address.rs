//! What counts as a web address where a form asks for one.
//!
//! Browsers' own `url` fields accept "https:example.org", reading it as
//! "https://example.org/", and a URL parser does the same, so neither is the
//! check. This is, here and in orrery's `SciStarter.is_url`, which follow the
//! same rule word for word.

/// Whether `text` is a whole web address: no whitespace anywhere, starting
/// with `http://` or `https://` (in any case) as written, parsing as a URL,
/// and naming a host of at least two labels, none empty. So not
/// "https:example.org", "https://", "https://example" or "xhttps://a.com".
pub fn is_web_address(text: &str) -> bool {
    if text.is_empty() || text.contains(char::is_whitespace) {
        return false;
    }

    let scheme = text.get(..8).unwrap_or(text).to_ascii_lowercase();
    if !scheme.starts_with("http://") && scheme != "https://" {
        return false;
    }

    let Ok(url) = url::Url::parse(text) else {
        return false;
    };

    url.host_str().is_some_and(|host| {
        let labels: Vec<&str> = host.split('.').collect();
        labels.len() >= 2 && labels.iter().all(|label| !label.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::is_web_address;

    #[test]
    fn a_whole_address_is_one() {
        for address in [
            "https://example.org",
            "http://example.org/event?id=1",
            "HTTPS://www.Example.org/a",
            "https://192.168.0.1/",
        ] {
            check!(is_web_address(address), "{address}");
        }
    }

    #[test]
    fn what_a_browser_would_take_is_not_enough() {
        for address in [
            "",
            "https:test.com",
            "https:/test.com",
            "https://",
            "https://test",
            "https://test.",
            "https://.test.com",
            "xhttps://a.com",
            "ftp://example.org",
            "example.org",
            "https://exa mple.org",
            " https://example.org",
        ] {
            check!(!is_web_address(address), "{address:?}");
        }
    }
}
