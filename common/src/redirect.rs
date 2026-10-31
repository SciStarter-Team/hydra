//! Where a visitor may be sent on to.

/// Whether a `next=` value may be redirected to.
///
/// A leading slash is not sufficient: `//evil.example` has one too, and a
/// browser reads it as a host rather than a path -- as it does `/\evil.example`,
/// since browsers treat a backslash in a URL as a slash. These values arrive
/// in links handed out by third parties, and a post-login redirect is a
/// convenient way to make a phishing page look like ours, so anything that is
/// not unambiguously a path on this site is refused.
pub fn is_local_next(next: &str) -> bool {
    next.starts_with('/') && !next.starts_with("//") && !next.starts_with("/\\")
}

#[cfg(test)]
mod tests {
    use super::is_local_next;

    #[test]
    fn a_next_is_a_path_here_and_not_another_host() {
        for good in ["/", "/dashboard", "/finder?q=birds", "/a//b"] {
            assert!(is_local_next(good), "{good:?} should be accepted");
        }

        for bad in [
            "//evil.example",
            "///evil.example",
            "//evil.example/x",
            "/\\evil.example",
            "https://evil.example/",
            "evil.example",
            "",
        ] {
            assert!(!is_local_next(bad), "{bad:?} should be refused");
        }
    }
}
