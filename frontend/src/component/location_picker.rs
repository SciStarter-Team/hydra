//! The location section of the add forms, which the event page's location
//! editor shows too: "Is it in person or virtual?" (for an event, "How will
//! people attend?"), a place search, the address, the region editor, the
//! location note and, for a project, its link. An event's webpage is asked
//! with its registration link instead.

use crate::{
    choice::{ChoiceList, ChoiceRadios, ChoiceValue},
    component::floating_field::FormErrors,
    project::location::PlaceOf,
};

/// The answers to an event's "Where does it take place?", asked once it is
/// in person: the location mode each chooses, its words, and the note under
/// them.
pub const PLACE_ANSWERS: [(&str, &str, &str); 3] = [
    (
        "in_person",
        "At one place",
        "A venue, park, or other address people come to",
    ),
    (
        "region",
        "Across an area",
        "Such as a city, park system, or watershed, drawn on a map",
    ),
    (
        "anywhere",
        "Anywhere in the world",
        "Wherever each person is, as in a worldwide bird count",
    ),
];

/// Reusable map-based area editor. Renders a MapLibre map plus the hidden
/// field it keeps in sync; the `js/region-editor.js` module (loaded by
/// `Head::with_region_editor`) wires up click-to-draw behavior. Drop this into
/// any form that needs to capture a `regions` geometry (e.g. the project form).
#[derive(sailfish::TemplateOnce)]
#[template(path = "site/region_editor.stpl.html")]
pub struct RegionEditor {
    /// Form field name the geometry is submitted under.
    field_name: String,
    /// DOM id of the hidden input (the map editor's `data-target`).
    field_id: String,
    /// Existing geometry as a GeoJSON string (empty for a fresh editor).
    initial: String,
}

/// The location section: an "in person or virtual?" mode select, a place
/// type-ahead search backed by `/add/place-search` with the structured address
/// fields behind a details disclosure, the region editor (revealed on request),
/// a free-text location note, and a project's link. Backed by
/// `js/location-picker.js`.
#[derive(sailfish::TemplateOnce)]
#[template(path = "site/location_picker.stpl.html")]
pub struct LocationPicker {
    mode: String,
    location_name: String,
    address: String,
    city: String,
    state: String,
    zip: String,
    country: String,
    latitude: String,
    longitude: String,
    location_description: String,
    /// The project's link; an event's webpage isn't asked here.
    url: String,
    /// What was left typed in the place search, kept when the form is
    /// refused so it can still be picked or used.
    place_query: String,
    region_editor: RegionEditor,
    /// Whether this is an event's location: where in person is asked
    /// (`PLACE_ANSWERS`), with the mode only an event has (anywhere in the
    /// world), a virtual event is asked no location note, and no link is
    /// asked.
    for_event: bool,
    /// The kind of venue an event held at an address is held at, one answer
    /// or none, asked under the location note.
    venues: Option<ChoiceRadios>,
    /// What's wrong with the form, when it was refused; marks the controls
    /// it is about.
    errors: FormErrors,
}

impl LocationPicker {
    /// Build from the common location fields shared by both forms.
    pub fn build(
        mode: &str,
        location_name: &str,
        address: &str,
        city: &str,
        state: &str,
        zip: &str,
        country: &str,
        latitude: &str,
        longitude: &str,
        location_description: &str,
        url: &str,
        regions_geojson: &str,
    ) -> Self {
        LocationPicker {
            mode: mode.to_string(),
            location_name: location_name.to_string(),
            address: address.to_string(),
            city: city.to_string(),
            state: state.to_string(),
            zip: zip.to_string(),
            country: country.to_string(),
            latitude: latitude.to_string(),
            longitude: longitude.to_string(),
            location_description: location_description.to_string(),
            url: url.to_string(),
            place_query: String::new(),
            region_editor: RegionEditor {
                field_name: "regions".to_string(),
                field_id: "regions-geojson".to_string(),
                initial: regions_geojson.to_string(),
            },
            for_event: false,
            venues: None,
            errors: FormErrors::new(),
        }
    }

    /// Show `query` in the place search, as it was left typed there.
    pub fn with_place_query(mut self, query: &str) -> Self {
        self.place_query = query.to_string();
        self
    }

    /// Ask what an event's location is: ask where an in-person one takes
    /// place, offering the mode only an event has, ask no location note of a
    /// virtual one, and ask no link.
    pub fn for_events(mut self) -> Self {
        self.for_event = true;
        self
    }

    /// Ask which of the kinds of venue `venues` an event held at an address
    /// is held at, if any, with `picked` (a slug, empty for none) chosen,
    /// when there are any.
    pub fn with_venues(mut self, venues: Vec<ChoiceValue>, picked: &str) -> Self {
        self.venues = (!venues.is_empty()).then(|| {
            ChoiceRadios::new(
                "venue",
                crate::event::choice_legend(ChoiceList::EventVenue),
                venues,
                crate::event::location::answered_venue(picked),
            )
        });
        self
    }

    /// Mark the controls `errors` are about, those that are this section's.
    pub fn errors(mut self, errors: &FormErrors) -> Self {
        self.errors = errors.clone();
        self
    }

    /// Whose location this is, which says what the words typed in the place
    /// search are used as.
    fn place_of(&self) -> PlaceOf {
        if self.for_event {
            PlaceOf::Event
        } else {
            PlaceOf::Project
        }
    }
}
