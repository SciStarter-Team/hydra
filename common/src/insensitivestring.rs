use std::{borrow::Borrow, fmt::Display, hash::Hash, ops::Deref};

/// A newtype for String which stores case (as String does) but does
/// hashing and comparison in a case-insensitive manner.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct InsensitiveString(String);

impl InsensitiveString {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<String> for InsensitiveString {
    fn from(value: String) -> Self {
        InsensitiveString(value)
    }
}

impl From<InsensitiveString> for String {
    fn from(value: InsensitiveString) -> Self {
        value.0
    }
}

impl From<&str> for InsensitiveString {
    fn from(value: &str) -> Self {
        InsensitiveString(value.to_string())
    }
}

impl AsRef<str> for InsensitiveString {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl Deref for InsensitiveString {
    type Target = <String as Deref>::Target;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl Display for InsensitiveString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Hash for InsensitiveString {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_ascii_lowercase().hash(state)
    }
}

impl PartialEq for InsensitiveString {
    fn eq(&self, other: &Self) -> bool {
        self.0
            .to_ascii_lowercase()
            .eq(&other.0.to_ascii_lowercase())
    }
}

impl PartialOrd for InsensitiveString {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.0
            .to_ascii_lowercase()
            .partial_cmp(&other.0.to_ascii_lowercase())
    }
}

impl Eq for InsensitiveString {}

impl Ord for InsensitiveString {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::InsensitiveString;

    #[test]
    fn test_eq_identical() {
        let a = InsensitiveString::from("moose");
        let b = InsensitiveString::from("moose");
        assert_eq!(a, b);
    }

    #[test]
    fn test_eq_case_difference() {
        let a = InsensitiveString::from("Moose");
        let b = InsensitiveString::from("moose");
        assert_eq!(a, b);
    }

    #[test]
    fn test_not_equal() {
        let a = InsensitiveString::from("moose");
        let b = InsensitiveString::from("antlers");
        assert_ne!(a, b);
    }

    #[test]
    fn test_ordering_case_difference() {
        let a = InsensitiveString::from("Moose");
        let b = InsensitiveString::from("antlers");
        assert!(a > b);
    }

    #[test]
    fn test_hash_set() {
        let a: HashSet<InsensitiveString> = [
            "Hey",
            "Bullwinkle",
            "do",
            "you",
            "want",
            "to",
            "see",
            "a",
            "magic",
            "trick",
            "?",
        ]
        .into_iter()
        .map(|s| InsensitiveString::from(s))
        .collect();

        let b: HashSet<InsensitiveString> = [
            "hey", "Dudley", "do", "you", "want", "to", "see", "a", "Magic", "trick", "?",
        ]
        .into_iter()
        .map(|s| InsensitiveString::from(s))
        .collect();

        let c: HashSet<_> = a.intersection(&b).cloned().collect();

        assert_eq!(c.len(), 10);
        assert!(c.contains(&"hey".into()));
        assert!(c.contains(&"Hey".into()));
        assert!(!c.contains(&"Dudley".into()));
        assert!(!c.contains(&"dudley".into()));
        assert!(!c.contains(&"Bullwinkle".into()));
        assert!(!c.contains(&"bullwinkle".into()));
    }
}
