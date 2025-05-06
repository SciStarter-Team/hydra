use jiff::Timestamp;

#[derive(
    Debug,
    Clone,
    Default,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    sqlx::Type,
    serde::Serialize,
    serde::Deserialize,
)]
#[sqlx(transparent)]
pub struct TimeString(String);

impl TryFrom<TimeString> for Timestamp {
    type Error = jiff::Error;

    fn try_from(value: TimeString) -> Result<Self, Self::Error> {
        value.0.parse()
    }
}

impl From<Timestamp> for TimeString {
    fn from(value: Timestamp) -> Self {
        TimeString(value.to_string())
    }
}

impl std::ops::Deref for TimeString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.0.as_str()
    }
}
