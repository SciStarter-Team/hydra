use std::fmt::Display;

use anyhow::{Error, anyhow};

/// An expression that can be evaluated to produce a Value
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    /// A variable lookup: `name`
    Variable(String),
    /// A string literal: `"hello"` or `'hello'`
    StringLiteral(String),
    /// An integer literal: `42`
    IntegerLiteral(i64),
}

impl Expression {
    /// Parse an expression from a string
    pub fn parse(s: &str) -> Result<Self, Error> {
        let s = s.trim();

        if s.is_empty() {
            return Err(anyhow!("Empty expression"));
        }

        // Check for string literal (double quotes)
        if s.starts_with('"') {
            if s.len() < 2 || !s.ends_with('"') {
                return Err(anyhow!("Unterminated string literal"));
            }
            let inner = &s[1..s.len() - 1];
            return Ok(Expression::StringLiteral(inner.to_string()));
        }

        // Check for string literal (single quotes)
        if s.starts_with('\'') {
            if s.len() < 2 || !s.ends_with('\'') {
                return Err(anyhow!("Unterminated string literal"));
            }
            let inner = &s[1..s.len() - 1];
            return Ok(Expression::StringLiteral(inner.to_string()));
        }

        // Check for integer literal (including negative)
        if s.starts_with('-') || s.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            if let Ok(n) = s.parse::<i64>() {
                return Ok(Expression::IntegerLiteral(n));
            }
        }

        // Otherwise it's a variable lookup
        Ok(Expression::Variable(s.to_string()))
    }

    /// Convert to owned version (for AST storage)
    pub fn to_owned_expr(&self) -> Expression {
        self.clone()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Integer(i64),
    List(Vec<Value>),
}

impl Value {
    pub fn truth(&self) -> bool {
        match self {
            Value::String(s) => !s.is_empty() && s != "false",
            Value::Integer(n) => *n != 0,
            Value::List(values) => !values.is_empty(),
        }
    }

    /// Write the value directly to a buffer (no allocation)
    pub fn write_to(&self, buffer: &mut Vec<u8>) {
        match self {
            Value::String(s) => {
                buffer.extend_from_slice(s.as_bytes());
            }
            Value::Integer(n) => {
                use std::io::Write;
                write!(buffer, "{}", n).ok();
            }
            Value::List(values) => {
                buffer.push(b'[');
                let mut first = true;
                for val in values {
                    if first {
                        first = false;
                    } else {
                        buffer.extend_from_slice(b", ");
                    }
                    val.write_to(buffer);
                }
                buffer.push(b']');
            }
        }
    }

    /// Write the value with HTML escaping directly to a buffer
    pub fn write_escaped(&self, buffer: &mut Vec<u8>) {
        match self {
            Value::String(s) => {
                html_escape_into(s, buffer);
            }
            Value::Integer(n) => {
                // Integers don't need escaping
                use std::io::Write;
                write!(buffer, "{}", n).ok();
            }
            Value::List(values) => {
                buffer.push(b'[');
                let mut first = true;
                for val in values {
                    if first {
                        first = false;
                    } else {
                        buffer.extend_from_slice(b", ");
                    }
                    val.write_escaped(buffer);
                }
                buffer.push(b']');
            }
        }
    }
}

/// Escape HTML special characters (optimized: bulk copy safe ranges)
fn html_escape_into(s: &str, buffer: &mut Vec<u8>) {
    let bytes = s.as_bytes();
    let mut last_safe = 0;

    for (i, &byte) in bytes.iter().enumerate() {
        let escape = match byte {
            b'&' => b"&amp;".as_slice(),
            b'<' => b"&lt;".as_slice(),
            b'>' => b"&gt;".as_slice(),
            b'"' => b"&quot;".as_slice(),
            b'\'' => b"&#x27;".as_slice(),
            _ => continue,
        };

        if last_safe < i {
            buffer.extend_from_slice(&bytes[last_safe..i]);
        }
        buffer.extend_from_slice(escape);
        last_safe = i + 1;
    }

    if last_safe < bytes.len() {
        buffer.extend_from_slice(&bytes[last_safe..]);
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::String(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::String(value.to_owned())
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Value::Integer(value)
    }
}

impl From<Vec<Value>> for Value {
    fn from(value: Vec<Value>) -> Self {
        Value::List(value)
    }
}

impl From<&[Value]> for Value {
    fn from(value: &[Value]) -> Self {
        let mut v: Vec<Value> = value.iter().cloned().collect();

        v.shrink_to_fit();

        Value::List(v)
    }
}

impl<const N: usize> From<[Value; N]> for Value {
    fn from(value: [Value; N]) -> Self {
        Value::List(value.into_iter().collect())
    }
}

impl Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::String(s) => s.fmt(f),
            Value::Integer(n) => n.fmt(f),
            Value::List(values) => {
                write!(f, "[")?;

                let mut first = true;

                for val in values.iter() {
                    if first {
                        first = false;
                    } else {
                        write!(f, ", ")?;
                    }

                    val.fmt(f)?;
                }

                write!(f, "]")
            }
        }
    }
}
