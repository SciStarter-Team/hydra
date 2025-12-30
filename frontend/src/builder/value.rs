use std::fmt::Display;

#[derive(Debug, Clone)]
pub enum Value {
    Single(String),
    List(Vec<Value>),
}

impl Value {
    pub fn truth(&self) -> bool {
        match self {
            Value::Single(s) => !s.is_empty() && s != "false",
            Value::List(values) => !values.is_empty(),
        }
    }

    /// Write the value directly to a buffer (no allocation)
    pub fn write_to(&self, buffer: &mut Vec<u8>) {
        match self {
            Value::Single(s) => {
                buffer.extend_from_slice(s.as_bytes());
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
            Value::Single(s) => {
                html_escape_into(s, buffer);
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
        Value::Single(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::Single(value.to_owned())
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
            Value::Single(s) => s.fmt(f),
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
