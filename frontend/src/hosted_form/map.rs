//! The public map of where a form's submissions were made, for a form that
//! has one: its latest submissions as GeoJSON, each with the popup its
//! marker opens, drawn here so that the page's script only places them.
//!
//! user-service decides what the map may show: only the answers to the
//! fields the map names, never who made a submission, and for a form whose
//! submissions are anonymized, places made vaguer.

use anyhow::Error;
use common::{
    message::form::{FormDefinition, FormLookup, MapMarker},
    service::request,
};
use jiff::{Timestamp, tz::TimeZone};
use poem::{Response, handler, http::StatusCode, web::Path};
use sailfish::TemplateOnce;
use ulid::Ulid;

use super::answers::{self, Display};

/// What a marker's popup shows.
#[derive(TemplateOnce)]
#[template(path = "hosted_form/map_popup.stpl.html")]
struct Popup<'a> {
    form: &'a FormDefinition,
    marker: &'a MapMarker,
}

impl Popup<'_> {
    /// The day the submission was made.
    fn day(&self) -> String {
        Timestamp::try_from(self.marker.when.clone())
            .map(|when| {
                when.to_zoned(TimeZone::UTC)
                    .strftime("%b %-d, %Y")
                    .to_string()
            })
            .unwrap_or_default()
    }

    /// Each answer the popup shows, in the map's order: the question, and
    /// the answer.
    fn rows(&self) -> Vec<(String, Display)> {
        let Some(map) = &self.form.map else {
            return Vec::new();
        };

        map.popup_fields
            .iter()
            .filter_map(|key| {
                let answer = self.marker.answers.get(key)?;
                let field = self.form.field(key.as_str());
                let label = field.map_or_else(|| key.to_string(), |field| field.label.clone());

                Some((label, answers::display(field, answer)))
            })
            .collect()
    }
}

/// GET `/-form/:form_id/map.json`: the form's map's markers, as a GeoJSON
/// feature collection whose features each carry the HTML of their popup.
#[handler]
pub(super) async fn markers(Path(form_id): Path<Ulid>) -> Result<Response, Error> {
    let Some(form) = super::load(FormLookup::Id(form_id)).await? else {
        return Ok(StatusCode::NOT_FOUND.into());
    };

    if form.map.is_none() {
        return Ok(StatusCode::NOT_FOUND.into());
    }

    let markers: Vec<MapMarker> = request("user.forms.map", form_id).await?;

    let features = markers
        .iter()
        .map(|marker| {
            Ok(serde_json::json!({
                "type": "Feature",
                "geometry": {
                    "type": "Point",
                    "coordinates": [marker.location.longitude, marker.location.latitude],
                },
                "properties": {
                    "popup": Popup { form: &form, marker }.render_once()?,
                },
            }))
        })
        .collect::<Result<Vec<_>, Error>>()?;

    let body = serde_json::json!({
        "type": "FeatureCollection",
        "features": features,
    });

    Ok(Response::builder()
        .content_type("application/geo+json")
        .body(body.to_string()))
}
