//! A project's answer to "where can you do this?", ready to display.
//!
//! The exchange shows this in three places -- the search card, the project
//! page and the event page -- and all build the value here, so they cannot
//! word the same place differently. Events carry the same location fields
//! as projects, spelled the same way.
//!
//! The wording is shared with orrery, whose `Where` type in `src/common.rs`
//! renders the same field on the same projects. The two are a cross-codebase
//! pair: if the phrasing changes in one it has to change in the other, or the
//! same project reads differently depending on which site is serving it.
//! What is *not* shared is the spelling of `location_type`: orrery stores the
//! codes `ANY`/`NEAR`/`AT`, and its sync translates them into the display
//! strings hydra stores, so each side matches on what it holds.

/// A project's location, as a reader should see it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Where {
    /// The project says nothing about where it happens.
    #[default]
    Unstated,

    Online,

    /// Doable anywhere. `landuse` names the kind of place the project asks
    /// people to visit, and is empty when the project didn't say.
    Anywhere {
        landuse: String,
    },

    /// Confined to an area. `coverage` is true when a polygon exists to draw.
    Area {
        coverage: bool,
        description: String,
    },

    /// Tied to one address.
    At {
        address: String,
    },
}

impl Where {
    /// Build the display from a project's raw location fields.
    ///
    /// `location_type` carries the display strings orrery's sync sends, and
    /// anything unrecognised -- including the empty string a project that
    /// never answered arrives with -- says nothing rather than guessing.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        location_type: &str,
        landuse: &str,
        description: &str,
        coverage: bool,
        city: &str,
        state: &str,
        country: &str,
        address: &str,
    ) -> Where {
        match location_type {
            "Online" => Where::Online,
            "Global, anywhere on the planet" => Where::Anywhere {
                landuse: landuse.trim().to_owned(),
            },
            "In a geographic location or range" => Where::Area {
                coverage,
                description: description.trim().to_owned(),
            },
            "At a specific address" => Where::At {
                address: Where::address_line(city, state, country, address),
            },
            _ => Where::Unstated,
        }
    }

    /// City and state together when both are known, else the most specific
    /// single field the project filled in.
    fn address_line(city: &str, state: &str, country: &str, address: &str) -> String {
        let (city, state) = (city.trim(), state.trim());

        if !city.is_empty() && !state.is_empty() {
            return format!("{city}, {state}");
        }

        [city, state, country.trim(), address.trim()]
            .into_iter()
            .find(|part| !part.is_empty())
            .unwrap_or_default()
            .to_owned()
    }

    /// The wording shown to a reader.
    ///
    /// Empty means there is nothing to say and the caller omits the row.
    pub fn label(&self) -> &str {
        match self {
            Where::Unstated => "",
            Where::Online => "Online",
            Where::Anywhere { landuse } if !landuse.is_empty() => landuse,
            Where::Anywhere { .. } => "Global, anywhere on the planet",
            Where::Area { description, .. } if !description.is_empty() => description,
            Where::Area { .. } => "In an area or range",
            Where::At { address } if !address.is_empty() => address,
            Where::At { .. } => "At a specific address",
        }
    }

    /// Whether a coverage polygon exists to offer as a map.
    pub fn has_map(&self) -> bool {
        matches!(self, Where::Area { coverage: true, .. })
    }
}

#[cfg(test)]
mod tests {
    use super::Where;

    fn location(location_type: &str) -> Where {
        Where::new(location_type, "", "", false, "", "", "", "")
    }

    #[test]
    fn unknown_location_types_say_nothing() {
        // The empty string is real data: projects that never answered arrive
        // from orrery's sync with it.
        assert_eq!(location("").label(), "");
        assert_eq!(location("somewhere new").label(), "");
        // The orrery-side spellings are not hydra's, and must not half-match.
        assert_eq!(location("ANY").label(), "");
        assert_eq!(location("NEAR").label(), "");
    }

    #[test]
    fn online_projects_say_online() {
        assert_eq!(location("Online").label(), "Online");
    }

    #[test]
    fn anywhere_prefers_the_landuse_the_project_gave() {
        let stated = Where::new(
            "Global, anywhere on the planet",
            "rivers and streams",
            "",
            false,
            "",
            "",
            "",
            "",
        );
        assert_eq!(stated.label(), "rivers and streams");

        assert_eq!(
            location("Global, anywhere on the planet").label(),
            "Global, anywhere on the planet"
        );
    }

    #[test]
    fn an_area_prefers_its_description_over_the_generic_phrase() {
        let described = Where::new(
            "In a geographic location or range",
            "",
            "Murraylands catchments",
            true,
            "",
            "",
            "",
            "",
        );
        assert_eq!(described.label(), "Murraylands catchments");

        assert_eq!(
            location("In a geographic location or range").label(),
            "In an area or range"
        );
    }

    #[test]
    fn only_an_area_with_a_polygon_offers_a_map() {
        let area = |coverage| {
            Where::new(
                "In a geographic location or range",
                "",
                "",
                coverage,
                "",
                "",
                "",
                "",
            )
        };

        assert!(area(true).has_map());
        assert!(!area(false).has_map());

        // A polygon left behind on a project that has since gone global is
        // not a map.
        let global = Where::new(
            "Global, anywhere on the planet",
            "",
            "",
            true,
            "",
            "",
            "",
            "",
        );
        assert!(!global.has_map());
    }

    #[test]
    fn an_address_pairs_city_with_state_and_otherwise_takes_what_it_has() {
        let at = |city, state, country, address| {
            Where::new(
                "At a specific address",
                "",
                "",
                false,
                city,
                state,
                country,
                address,
            )
        };

        assert_eq!(
            at("Adelaide", "SA", "Australia", "1 North Tce").label(),
            "Adelaide, SA"
        );
        assert_eq!(
            at("Adelaide", "", "Australia", "1 North Tce").label(),
            "Adelaide"
        );
        assert_eq!(at("", "", "Australia", "1 North Tce").label(), "Australia");
        assert_eq!(at("", "", "", "1 North Tce").label(), "1 North Tce");
        assert_eq!(at("", "", "", "").label(), "At a specific address");
    }

    #[test]
    fn whitespace_only_fields_count_as_absent() {
        let padded = Where::new(
            "At a specific address",
            "",
            "",
            false,
            "  ",
            " ",
            "Australia",
            "",
        );
        assert_eq!(padded.label(), "Australia");

        let landuse = Where::new(
            "Global, anywhere on the planet",
            "   ",
            "",
            false,
            "",
            "",
            "",
            "",
        );
        assert_eq!(landuse.label(), "Global, anywhere on the planet");
    }
}
