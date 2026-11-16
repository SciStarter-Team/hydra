//! The time-zone control of a date/time row.

use crate::component::floating_field::FormError;

/// Time zones offered in the start-time picker. The page's script adds the
/// visitor's actual browser time zone if it isn't already listed and
/// preselects it, so this curated list only needs to cover common cases.
const COMMON_TIMEZONES: &[(&str, &str)] = &[
    ("America/New_York", "Eastern Time (US & Canada)"),
    ("America/Chicago", "Central Time (US & Canada)"),
    ("America/Denver", "Mountain Time (US & Canada)"),
    ("America/Phoenix", "Mountain Time - Arizona (no DST)"),
    ("America/Los_Angeles", "Pacific Time (US & Canada)"),
    ("America/Anchorage", "Alaska Time"),
    ("Pacific/Honolulu", "Hawaii Time"),
    ("America/Toronto", "Eastern Time - Toronto"),
    ("America/Mexico_City", "Central Time - Mexico City"),
    ("UTC", "UTC"),
    ("Europe/London", "London (GMT/BST)"),
    ("Europe/Paris", "Central European Time"),
    ("Europe/Berlin", "Central European Time - Berlin"),
    ("Africa/Johannesburg", "South Africa Standard Time"),
    ("Asia/Kolkata", "India Standard Time"),
    ("Asia/Shanghai", "China Standard Time"),
    ("Asia/Tokyo", "Japan Standard Time"),
    ("Australia/Sydney", "Australian Eastern Time"),
];

/// Every IANA time zone, grouped by region for the picker's `<optgroup>`s.
/// The curated `COMMON_TIMEZONES` stay at the top of the select; these groups
/// make the rest of the world reachable. Entries are `(value, label)` where
/// the label drops the region prefix and underscores ("America/Argentina/
/// Buenos_Aires" -> "Argentina/Buenos Aires" under "America"). Built once.
fn all_timezones() -> &'static [(String, Vec<(String, String)>)] {
    static ZONES: std::sync::OnceLock<Vec<(String, Vec<(String, String)>)>> =
        std::sync::OnceLock::new();
    ZONES.get_or_init(|| {
        const REGIONS: &[&str] = &[
            "Africa",
            "America",
            "Antarctica",
            "Asia",
            "Atlantic",
            "Australia",
            "Europe",
            "Indian",
            "Pacific",
        ];
        let mut groups: Vec<(String, Vec<(String, String)>)> = REGIONS
            .iter()
            .map(|region| (region.to_string(), Vec::new()))
            .collect();

        let mut names: Vec<String> = jiff::tz::db()
            .available()
            .map(|name| name.to_string())
            .collect();
        names.sort();

        for name in names {
            // Region-less names (UTC, EST5EDT, ...) and non-geographic
            // regions (Etc/*, US/*, ...) are aliases or legacy entries; the
            // canonical geographic zones all live under the regions above.
            let Some((region, rest)) = name.split_once('/') else {
                continue;
            };
            if let Some(group) = groups.iter_mut().find(|(r, _)| r == region) {
                let label = rest.replace('_', " ");
                group.1.push((name.clone(), label));
            }
        }
        groups
    })
}

/// The time-zone control for one date/time row, a field in the row beside
/// the date and time. Zones are inferred from the picked venue or the
/// browser, so the list is there to correct an inference rather than to ask a
/// question. With JavaScript the field shows the zone's short name ("PDT",
/// "GMT+3") over the full list, which opens on click; without it, the list
/// itself shows.
#[derive(sailfish::TemplateOnce)]
#[template(path = "site/timezone_field.stpl.html")]
pub struct TimezoneField {
    field_name: String,
    selected: String,
    required: bool,
    invalid: bool,
    timezones: &'static [(&'static str, &'static str)],
    all_timezones: &'static [(String, Vec<(String, String)>)],
}

impl TimezoneField {
    /// Build the control for `field_name`, preselecting `selected`.
    pub fn build(field_name: impl Into<String>, selected: &str, required: bool) -> Self {
        TimezoneField {
            field_name: field_name.into(),
            selected: selected.to_string(),
            required,
            invalid: false,
            timezones: COMMON_TIMEZONES,
            all_timezones: all_timezones(),
        }
    }

    /// Mark the field invalid when `error` is about it.
    pub fn errors(mut self, error: Option<&FormError>) -> Self {
        self.invalid = error.is_some_and(|e| e.is_about(&self.field_name));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A venue pick's inferred zone must be a real IANA zone that the form's
    /// time-zone select actually offers, or the client-side defaulting is a
    /// silent no-op.
    #[test]
    fn venue_timezone_is_selectable() {
        // Philadelphia city hall.
        let zone = crate::event::zone::name_at(39.9527, -75.1635);
        assert_eq!(zone, "America/New_York");

        let listed = all_timezones()
            .iter()
            .any(|(_, zones)| zones.iter().any(|(value, _)| *value == zone));
        assert!(listed, "inferred zone {zone} missing from the tz select");
    }
}
