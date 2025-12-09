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
