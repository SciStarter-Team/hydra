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

/// Parse key=value attributes from a string with no leading name token.
///
/// [`parse_attributes`] reserves the first token for a name, which is what a
/// slot or a widget directive starts with. Where the caller has already taken
/// whatever precedes the attributes, this reads what is left without eating a
/// key=value pair to do it.
pub fn parse_attributes_only(input: &str) -> AttributeParser<'_> {
    AttributeParser {
        input,
        bytes: input.as_bytes(),
        pos: 0,
        first_token_consumed: true,
    }
}

/// Read a leading quoted string, returning it unescaped along with whatever
/// follows the closing quote.
///
/// The quoting rules are an attribute value's: either quote character
/// delimits, and a backslash escapes the character after it. `None` means the
/// input did not start with a quote, or the quote was never closed.
pub fn parse_quoted(input: &str) -> Option<(String, &str)> {
    let input = input.trim_start();
    let mut chars = input.char_indices();
    let (_, quote) = chars.next()?;

    if quote != '"' && quote != '\'' {
        return None;
    }

    while let Some((index, c)) = chars.next() {
        if c == '\\' {
            // Skips the escaped character whole, so a multi-byte one after a
            // backslash cannot leave the scan inside a character.
            chars.next();
        } else if c == quote {
            // Both quote characters are one byte, so the arithmetic here
            // stays on character boundaries.
            return Some((unescape_value(&input[1..index]), &input[index + 1..]));
        }
    }

    None
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

                // Find the closing quote, stepping over escape sequences.
                //
                // This scans bytes rather than characters, and skipping an
                // escape two bytes at a time can land in the middle of a
                // multi-byte one. That is safe, but only because of a
                // property of UTF-8 worth writing down: every byte of a
                // multi-byte character is 0x80 or above, so none of them can
                // be mistaken for a quote or a backslash. A scan that lands
                // inside a character therefore walks back out of it without
                // matching anything, and `value_end` is only ever set at a
                // quote or at the end of the input -- both of which are
                // character boundaries, so the slice below cannot split one.
                while self.pos < self.bytes.len() {
                    if self.bytes[self.pos] == b'\\' && self.pos + 1 < self.bytes.len() {
                        self.pos += 2;
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
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars();

    while let Some(c) = chars.next() {
        if c == '\\' {
            // A backslash escapes the next character; a trailing backslash with
            // nothing after it is kept literally.
            match chars.next() {
                Some(next) => result.push(next),
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
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
        assert_eq!(
            attrs[0],
            ParsedAttr {
                key: "foo",
                value: "bar"
            }
        );
        assert_eq!(
            attrs[1],
            ParsedAttr {
                key: "baz",
                value: "qux"
            }
        );
    }

    #[test]
    fn test_parse_quoted_values() {
        let mut parser = parse_attributes(r#"hero text="This is a paragraph, yo." count=5"#);
        assert_eq!(parser.name(), Some("hero"));

        let attrs: Vec<_> = parser.collect();
        assert_eq!(attrs.len(), 2);
        assert_eq!(
            attrs[0],
            ParsedAttr {
                key: "text",
                value: "This is a paragraph, yo."
            }
        );
        assert_eq!(
            attrs[1],
            ParsedAttr {
                key: "count",
                value: "5"
            }
        );
    }

    #[test]
    fn test_parse_single_quoted_values() {
        let mut parser = parse_attributes("hero text='Hello World'");
        assert_eq!(parser.name(), Some("hero"));

        let attrs: Vec<_> = parser.collect();
        assert_eq!(attrs.len(), 1);
        assert_eq!(
            attrs[0],
            ParsedAttr {
                key: "text",
                value: "Hello World"
            }
        );
    }

    #[test]
    fn test_parse_escaped_quotes() {
        let mut parser = parse_attributes(r#"hero text="Hello \"World\"""#);
        assert_eq!(parser.name(), Some("hero"));

        let attrs: Vec<_> = parser.collect();
        assert_eq!(attrs.len(), 1);
        assert_eq!(
            attrs[0],
            ParsedAttr {
                key: "text",
                value: r#"Hello \"World\""#
            }
        );
    }

    #[test]
    fn test_unescape_value() {
        assert_eq!(unescape_value(r#"Hello \"World\""#), "Hello \"World\"");
        assert_eq!(unescape_value(r#"foo\\bar"#), "foo\\bar");
        assert_eq!(unescape_value("no escapes"), "no escapes");
        // Multi-byte UTF-8 must survive unescaping intact (regression: byte-wise
        // iteration decoded each UTF-8 byte as Latin-1, mangling "español").
        assert_eq!(
            unescape_value(r#"También disponible en español. \"sí\""#),
            "También disponible en español. \"sí\""
        );
        // A trailing lone backslash is preserved.
        assert_eq!(unescape_value(r#"ends with\"#), r#"ends with\"#);
    }

    /// The escape skip moves two bytes, so an escaped multi-byte character
    /// leaves it part-way into one. Nothing may be mangled or split by that.
    #[test]
    fn an_escape_before_a_multibyte_character_parses_intact() {
        let attrs: Vec<_> = parse_attributes(r#"hero text="Se llama \María" count=5"#).collect();

        assert_eq!(attrs.len(), 2);
        assert_eq!(unescape_value(attrs[0].value), "Se llama María");
        assert_eq!(attrs[1].value, "5");
    }

    /// The same, for a character wide enough to leave two bytes behind.
    #[test]
    fn an_escape_before_a_four_byte_character_parses_intact() {
        let attrs: Vec<_> = parse_attributes(r#"hero text="party \🎉 time" count=5"#).collect();

        assert_eq!(attrs.len(), 2);
        assert_eq!(unescape_value(attrs[0].value), "party 🎉 time");
        assert_eq!(attrs[1].value, "5");
    }

    /// A value ending in a backslash has nothing to escape, so the scan must
    /// not read past the end looking for it.
    #[test]
    fn a_value_may_end_in_a_backslash() {
        let attrs: Vec<_> = parse_attributes(r#"hero text="ends with\"#).collect();

        assert_eq!(attrs.len(), 1);
        assert_eq!(attrs[0].value, r#"ends with\"#);
    }

    #[test]
    fn attributes_only_keeps_the_first_pair() {
        let attrs: Vec<_> = parse_attributes_only(r#"context="verb" other=1"#).collect();

        assert_eq!(attrs.len(), 2);
        assert_eq!(
            attrs[0],
            ParsedAttr {
                key: "context",
                value: "verb"
            }
        );
    }

    #[test]
    fn quoted_prefix_is_taken_and_the_rest_left() {
        assert_eq!(
            parse_quoted(r#""Log In or Sign Up" context="verb""#),
            Some((String::from("Log In or Sign Up"), r#" context="verb""#))
        );
    }

    #[test]
    fn a_quoted_prefix_may_contain_its_own_quote() {
        assert_eq!(
            parse_quoted(r#""She said \"hi\"" rest"#),
            Some((String::from(r#"She said "hi""#), " rest"))
        );
    }

    #[test]
    fn single_quotes_delimit_too() {
        assert_eq!(
            parse_quoted("'Sign Up' rest"),
            Some((String::from("Sign Up"), " rest"))
        );
    }

    /// An escaped multi-byte character must not leave the scan part-way into
    /// a character, which would panic on the slice that follows.
    #[test]
    fn an_escape_before_a_multibyte_character_is_survivable() {
        assert_eq!(
            parse_quoted(r#""Se llama \María" rest"#),
            Some((String::from("Se llama María"), " rest"))
        );
    }

    #[test]
    fn unquoted_or_unterminated_input_is_not_a_quoted_string() {
        assert_eq!(parse_quoted("bare words"), None);
        assert_eq!(parse_quoted(r#""never closed"#), None);
        assert_eq!(parse_quoted(""), None);
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
