//! Shared parsing utilities for template syntax

/// Parsed key-value attribute
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedAttr<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

/// Parse key=value attributes from a string, respecting quoted values.
///
/// Supports:
/// - Unquoted values: `key=value`
/// - Double-quoted values: `key="value with spaces"`
/// - Single-quoted values: `key='value with spaces'`
/// - Escaped quotes within quoted values: `key="say \"hello\""`
///
/// Returns an iterator over the first token (name) and subsequent key=value pairs.
pub fn parse_attributes(input: &str) -> AttributeParser<'_> {
    AttributeParser {
        input,
        bytes: input.as_bytes(),
        pos: 0,
        first_token_consumed: false,
    }
}

pub struct AttributeParser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
    first_token_consumed: bool,
}

impl<'a> AttributeParser<'a> {
    /// Get the first token (typically the name) before any key=value pairs
    pub fn name(&mut self) -> Option<&'a str> {
        if self.first_token_consumed {
            return None;
        }
        self.first_token_consumed = true;

        // Skip leading whitespace
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }

        if self.pos >= self.bytes.len() {
            return None;
        }

        // Extract first token
        let start = self.pos;
        while self.pos < self.bytes.len() && !self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }

        if self.pos == start {
            None
        } else {
            Some(&self.input[start..self.pos])
        }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }
}

impl<'a> Iterator for AttributeParser<'a> {
    type Item = ParsedAttr<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        // Ensure first token is consumed
        if !self.first_token_consumed {
            self.name();
        }

        loop {
            self.skip_whitespace();
            if self.pos >= self.bytes.len() {
                return None;
            }

            // Extract key (up to '=')
            let key_start = self.pos;
            while self.pos < self.bytes.len()
                && self.bytes[self.pos] != b'='
                && !self.bytes[self.pos].is_ascii_whitespace()
            {
                self.pos += 1;
            }

            if self.pos >= self.bytes.len() || self.bytes[self.pos] != b'=' {
                // Not a key=value pair, skip this token
                while self.pos < self.bytes.len() && !self.bytes[self.pos].is_ascii_whitespace() {
                    self.pos += 1;
                }
                continue;
            }

            let key = &self.input[key_start..self.pos];
            self.pos += 1; // Skip '='

            if self.pos >= self.bytes.len() {
                return None;
            }

            // Extract value - check for quotes
            let value = if self.bytes[self.pos] == b'"' || self.bytes[self.pos] == b'\'' {
                let quote = self.bytes[self.pos];
                self.pos += 1; // Skip opening quote
                let value_start = self.pos;

                // Find closing quote, handling escapes
                while self.pos < self.bytes.len() {
                    if self.bytes[self.pos] == b'\\' && self.pos + 1 < self.bytes.len() {
                        self.pos += 2; // Skip escape sequence
                        continue;
                    }
                    if self.bytes[self.pos] == quote {
                        break;
                    }
                    self.pos += 1;
                }
                let value_end = self.pos;
                if self.pos < self.bytes.len() {
                    self.pos += 1; // Skip closing quote
                }
                &self.input[value_start..value_end]
            } else {
                // Unquoted value - read until whitespace
                let value_start = self.pos;
                while self.pos < self.bytes.len() && !self.bytes[self.pos].is_ascii_whitespace() {
                    self.pos += 1;
                }
                &self.input[value_start..self.pos]
            };

            return Some(ParsedAttr { key, value });
        }
    }
}

/// Unescape a string value that may contain escape sequences like \" or \\
pub fn unescape_value(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut result = String::with_capacity(input.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 1;
            result.push(bytes[i] as char);
        } else {
            result.push(bytes[i] as char);
        }
        i += 1;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_attrs() {
        let mut parser = parse_attributes("main foo=bar baz=qux");
        assert_eq!(parser.name(), Some("main"));

        let attrs: Vec<_> = parser.collect();
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0], ParsedAttr { key: "foo", value: "bar" });
        assert_eq!(attrs[1], ParsedAttr { key: "baz", value: "qux" });
    }

    #[test]
    fn test_parse_quoted_values() {
        let mut parser = parse_attributes(r#"hero text="This is a paragraph, yo." count=5"#);
        assert_eq!(parser.name(), Some("hero"));

        let attrs: Vec<_> = parser.collect();
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0], ParsedAttr { key: "text", value: "This is a paragraph, yo." });
        assert_eq!(attrs[1], ParsedAttr { key: "count", value: "5" });
    }

    #[test]
    fn test_parse_single_quoted_values() {
        let mut parser = parse_attributes("hero text='Hello World'");
        assert_eq!(parser.name(), Some("hero"));

        let attrs: Vec<_> = parser.collect();
        assert_eq!(attrs.len(), 1);
        assert_eq!(attrs[0], ParsedAttr { key: "text", value: "Hello World" });
    }

    #[test]
    fn test_parse_escaped_quotes() {
        let mut parser = parse_attributes(r#"hero text="Hello \"World\"""#);
        assert_eq!(parser.name(), Some("hero"));

        let attrs: Vec<_> = parser.collect();
        assert_eq!(attrs.len(), 1);
        assert_eq!(attrs[0], ParsedAttr { key: "text", value: r#"Hello \"World\""# });
    }

    #[test]
    fn test_unescape_value() {
        assert_eq!(unescape_value(r#"Hello \"World\""#), "Hello \"World\"");
        assert_eq!(unescape_value(r#"foo\\bar"#), "foo\\bar");
        assert_eq!(unescape_value("no escapes"), "no escapes");
    }

    #[test]
    fn test_parse_name_only() {
        let mut parser = parse_attributes("main");
        assert_eq!(parser.name(), Some("main"));
        assert_eq!(parser.next(), None);
    }

    #[test]
    fn test_parse_empty() {
        let mut parser = parse_attributes("");
        assert_eq!(parser.name(), None);
    }
}
