//! Email addresses as a shared concern.

/// Whether `address` is plausibly a deliverable email address.
///
/// This is a structural check, not RFC 5322: it rejects the shapes that are
/// certainly wrong -- empty, no `@`, an empty local or domain part, a domain
/// with no dot, whitespace -- and accepts everything else. Whether an address
/// actually receives mail can only be settled by sending to it.
///
/// Every service that stores an address validates with this, so that a client
/// which skips its own checks cannot write something unusable. An empty address
/// in particular is worse than useless: it cannot be named in a URL, so the
/// record it creates cannot be addressed afterwards.
pub fn valid(address: &str) -> bool {
    let address = address.trim();

    if address.contains(char::is_whitespace) || address.matches('@').count() != 1 {
        return false;
    }

    let mut parts = address.splitn(2, '@');
    match (parts.next(), parts.next()) {
        (Some(local), Some(domain)) => {
            !local.is_empty()
                && !domain.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::valid;

    #[test]
    fn ordinary_addresses_are_accepted() {
        assert!(valid("someone@example.com"));
        assert!(valid("first.last+tag@sub.example.co.uk"));
        assert!(valid("  padded@example.com  "));
    }

    /// The case that prompted this: an empty address was stored, and the record
    /// could not then be named in a URL to delete it.
    #[test]
    fn an_empty_address_is_rejected() {
        assert!(!valid(""));
        assert!(!valid("   "));
    }

    #[test]
    fn structurally_broken_addresses_are_rejected() {
        assert!(!valid("no-at-sign"));
        assert!(!valid("@example.com"));
        assert!(!valid("someone@"));
        assert!(!valid("someone@nodot"));
        assert!(!valid("someone@.example.com"));
        assert!(!valid("someone@example.com."));
        assert!(!valid("two@at@example.com"));
        assert!(!valid("has space@example.com"));
    }
}
