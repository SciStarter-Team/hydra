use anyhow::Error;

/// Represents the source of a project field value, used for permission checking.
/// Higher priority sources can overwrite lower priority sources, but not vice versa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FieldSource {
    /// Data imported from the legacy system (lowest priority)
    #[default]
    Legacy = 1,
    /// Data from API or third-party partner integrations
    Api = 2,
    /// Data entered via web form by project owners
    Form = 3,
    /// Data entered by SciStarter staff (highest priority)
    Staff = 4,
}

impl FieldSource {
    /// Returns the priority level of this source (higher = more authoritative)
    pub fn priority(&self) -> u8 {
        *self as u8
    }

    /// Returns true if this source can overwrite a field currently controlled by `existing`
    pub fn can_overwrite(&self, existing: FieldSource) -> bool {
        self.priority() >= existing.priority()
    }

    /// Returns the database string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            FieldSource::Legacy => "Legacy",
            FieldSource::Api => "Api",
            FieldSource::Form => "Form",
            FieldSource::Staff => "Staff",
        }
    }
}

impl TryFrom<&str> for FieldSource {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "Legacy" => Ok(FieldSource::Legacy),
            "Api" => Ok(FieldSource::Api),
            "Form" => Ok(FieldSource::Form),
            "Staff" => Ok(FieldSource::Staff),
            _ => Err(anyhow::anyhow!("Unknown FieldSource: {}", s)),
        }
    }
}

impl std::fmt::Display for FieldSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        assert!(FieldSource::Staff.priority() > FieldSource::Form.priority());
        assert!(FieldSource::Form.priority() > FieldSource::Api.priority());
        assert!(FieldSource::Api.priority() > FieldSource::Legacy.priority());
    }

    #[test]
    fn test_can_overwrite() {
        // Same priority can overwrite
        assert!(FieldSource::Staff.can_overwrite(FieldSource::Staff));
        assert!(FieldSource::Legacy.can_overwrite(FieldSource::Legacy));

        // Higher priority can overwrite lower
        assert!(FieldSource::Staff.can_overwrite(FieldSource::Form));
        assert!(FieldSource::Staff.can_overwrite(FieldSource::Api));
        assert!(FieldSource::Staff.can_overwrite(FieldSource::Legacy));
        assert!(FieldSource::Form.can_overwrite(FieldSource::Api));
        assert!(FieldSource::Form.can_overwrite(FieldSource::Legacy));
        assert!(FieldSource::Api.can_overwrite(FieldSource::Legacy));

        // Lower priority cannot overwrite higher
        assert!(!FieldSource::Legacy.can_overwrite(FieldSource::Api));
        assert!(!FieldSource::Legacy.can_overwrite(FieldSource::Form));
        assert!(!FieldSource::Legacy.can_overwrite(FieldSource::Staff));
        assert!(!FieldSource::Api.can_overwrite(FieldSource::Form));
        assert!(!FieldSource::Api.can_overwrite(FieldSource::Staff));
        assert!(!FieldSource::Form.can_overwrite(FieldSource::Staff));
    }

    #[test]
    fn test_roundtrip() {
        for source in [
            FieldSource::Legacy,
            FieldSource::Api,
            FieldSource::Form,
            FieldSource::Staff,
        ] {
            let s = source.as_str();
            let parsed = FieldSource::try_from(s).unwrap();
            assert_eq!(source, parsed);
        }
    }
}
