//! The location section of the add forms, which the event page's location
//! editor shows too: "Is it in person or virtual?", a place search, the
//! address, the region editor, the location note and the link, and, for a
//! virtual event, how people join it.

use crate::{
    choice::{ChoiceCheckboxes, ChoiceList},
    component::floating_field::FormError,
};

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
/// a free-text location note, and the link/URL. Backed by
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
    url: String,
    /// How people join a virtual event, `link` or `other`, or empty until it
    /// is chosen, and the instructions "other" asks for. Asked of events
    /// alone.
    virtual_join: String,
    how_to_join: String,
    /// What was left typed in the place search, kept when the form is
    /// refused so it can still be picked or used as the address.
    place_query: String,
    region_editor: RegionEditor,
    /// Whether the mode only an event has is offered: in person, anywhere
    /// in the world.
    offers_event_modes: bool,
    /// Whether the location note is asked of a place joined online; an
    /// online event has none.
    notes_online: bool,
    /// The kinds of venue an event held at an address may pick, asked with
    /// the address.
    venues: Option<ChoiceCheckboxes>,
    /// The form's error, when it was refused; marks the control it is about.
    error: Option<FormError>,
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
            virtual_join: String::new(),
            how_to_join: String::new(),
            place_query: String::new(),
            region_editor: RegionEditor {
                field_name: "regions".to_string(),
                field_id: "regions-geojson".to_string(),
                initial: regions_geojson.to_string(),
            },
            offers_event_modes: false,
            notes_online: true,
            venues: None,
            error: None,
        }
    }

    /// Show `query` in the place search, as it was left typed there.
    pub fn with_place_query(mut self, query: &str) -> Self {
        self.place_query = query.to_string();
        self
    }

    /// Ask what an event's location is: offer the modes only an event has
    /// besides the others, ask no location note of a virtual one, and ask
    /// how people join it, `virtual_join` (`link` or `other`) with the
    /// instructions `how_to_join` that "other" asks for.
    pub fn for_events(mut self, virtual_join: &str, how_to_join: &str) -> Self {
        self.offers_event_modes = true;
        self.notes_online = false;
        self.virtual_join = virtual_join.to_string();
        self.how_to_join = how_to_join.to_string();
        self
    }

    /// Ask which kinds of `venues` an event held at an address is held at,
    /// when there are any.
    pub fn with_venues(mut self, venues: ChoiceCheckboxes) -> Self {
        self.venues = (!venues.is_empty()).then(|| {
            venues.with_legend(format!("{} (optional)", ChoiceList::EventVenue.heading()))
        });
        self
    }

    /// Mark the control `error` is about, when it is one of this section's.
    pub fn errors(mut self, error: Option<&FormError>) -> Self {
        self.error = error.cloned();
        self
    }

    /// Whether the form's error is about the control named `field`.
    fn is_invalid(&self, field: &str) -> bool {
        self.error.as_ref().is_some_and(|e| e.is_about(field))
    }
}
