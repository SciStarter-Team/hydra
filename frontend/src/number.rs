//! Presentation of numbers.

/// Format an integer with comma thousands separators (`1234567` → `1,234,567`).
pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);

    if n < 0 {
        out.push('-');
    }

    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_groups_digits() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(-4_321), "-4,321");
    }

    /// `i64::MIN` has no positive counterpart, so negating it would panic.
    #[test]
    fn thousands_handles_the_most_negative_integer() {
        assert_eq!(thousands(i64::MIN), "-9,223,372,036,854,775,808");
    }
}
