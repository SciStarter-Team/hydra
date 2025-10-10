use aho_corasick::AhoCorasick;
use anyhow::Error;

/// Merge template with field values using aho-corasick for efficient multi-pattern replacement
pub fn merge_template(template: &str, field_names: &[String], field_values: &[String]) -> Result<String, Error> {
    if field_names.len() != field_values.len() {
        anyhow::bail!("Field names and values must have the same length");
    }

    // Build patterns like {{FIELD_NAME}}
    let patterns: Vec<String> = field_names
        .iter()
        .map(|name| format!("{{{{{}}}}}", name))
        .collect();

    // Build AhoCorasick automaton
    let ac = AhoCorasick::new(&patterns)?;

    // Replace all patterns with their values
    let result = ac.replace_all(template, field_values);

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_template() {
        let template = "Hello {{FIRST_NAME}}, your email is {{EMAIL}}";
        let field_names = vec!["FIRST_NAME".to_string(), "EMAIL".to_string()];
        let field_values = vec!["Alice".to_string(), "alice@example.com".to_string()];

        let result = merge_template(template, &field_names, &field_values).unwrap();
        assert_eq!(result, "Hello Alice, your email is alice@example.com");
    }

    #[test]
    fn test_merge_template_missing_fields() {
        let template = "Hello {{FIRST_NAME}}, {{MISSING}} field";
        let field_names = vec!["FIRST_NAME".to_string()];
        let field_values = vec!["Bob".to_string()];

        let result = merge_template(template, &field_names, &field_values).unwrap();
        // Missing fields remain unchanged
        assert_eq!(result, "Hello Bob, {{MISSING}} field");
    }
}
