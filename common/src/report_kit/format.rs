//! Text formatting for report pages and charts. **Shared file**; see
//! `report_kit.rs`.

/// A count with thousands separators: `2,111`.
pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();

    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }

    if n < 0 { format!("-{out}") } else { out }
}

/// Escapes text for HTML and SVG content and attribute values.
pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separates_groups_of_three() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(944), "944");
        assert_eq!(thousands(2111), "2,111");
        assert_eq!(thousands(1234567), "1,234,567");
        assert_eq!(thousands(-1500), "-1,500");
    }

    #[test]
    fn markup_characters_are_escaped() {
        assert_eq!(
            escape(r#"<b a="x">&</b>"#),
            "&lt;b a=&quot;x&quot;&gt;&amp;&lt;/b&gt;"
        );
    }
}
