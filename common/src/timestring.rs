use std::str::FromStr;

use jiff::Timestamp;

#[cfg_attr(feature = "database", derive(sqlx::Type))]
#[derive(
    Debug,
    Clone,
    Default,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    serde::Serialize,
    serde::Deserialize,
)]
#[cfg_attr(feature = "database", sqlx(transparent))]
pub struct TimeString(String);

impl TimeString {
    pub fn now() -> TimeString {
        TimeString(Timestamp::now().to_string())
    }
}

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

impl TryFrom<String> for TimeString {
    type Error = <Timestamp as FromStr>::Err;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let ts: Timestamp = value.parse()?;
        Ok(TimeString(ts.to_string()))
    }
}

impl From<TimeString> for String {
    fn from(value: TimeString) -> Self {
        value.0
    }
}
