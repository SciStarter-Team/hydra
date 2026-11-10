//! Turning a human-facing name into a URL- and key-safe slug.

/// The name, lowercased, with every run of characters other than ASCII letters
/// and digits made a single dash, and no dash at either end. A name with no
/// letters or digits in it gives the empty string.
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_dash = false;
    for c in input.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    #[test]
    fn runs_of_other_characters_become_one_dash() {
        check!(slugify("Hero Banner") == "hero-banner");
        check!(slugify("  Odd / Name!! ") == "odd-name");
        check!(slugify("talk/science cafe") == "talk-science-cafe");
        check!(slugify("MixedCase_123") == "mixedcase-123");
    }

    #[test]
    fn a_name_without_letters_or_digits_has_an_empty_slug() {
        check!(slugify("***") == "");
        check!(slugify("") == "");
    }
}
