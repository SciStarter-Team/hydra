//! Photos for a form's photo fields, uploaded as each is chosen, before the
//! form is sent. The form then sends the uploaded photos' addresses, which
//! must be uploads of this site's ([`unhosted`]).
//!
//! Anyone a form takes submissions from may upload photos for it, signed in
//! or not, as orrery let them: a form open to everyone takes photos from
//! everyone.

use anyhow::Error;
use common::message::form::{FieldError, FieldKind, FormDefinition, FormLookup, RawAnswers};
use poem::{
    Response, handler,
    web::{Multipart, Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use super::page::{control_name, field_id};
use crate::{middleware::authorization::Authenticated, upload::storage::untracked_file_id};

const DOMAIN: &str = env!("DOMAIN");

/// The address of an untracked upload whose path is `path`, as a form keeps
/// it: whole, so that it can be followed from wherever the submission goes.
fn hosted_url(path: &str) -> String {
    format!("https://{DOMAIN}{path}")
}

/// The upload of this site's that `url` is the address of, if it is one.
fn hosted_upload(url: &str) -> Option<Ulid> {
    untracked_file_id(url.strip_prefix(&format!("https://{DOMAIN}"))?)
}

/// The refusals of photo answers in `answers` that are not uploads of this
/// site's: a photo is uploaded here as it is chosen, so anything else was
/// not chosen on the form's page.
pub(super) fn unhosted(form: &FormDefinition, answers: &RawAnswers) -> Vec<FieldError> {
    form.fields
        .iter()
        .filter(|field| matches!(field.kind, FieldKind::Photo { .. }))
        .filter(|field| {
            answers.get(field.key.as_str()).is_some_and(|urls| {
                urls.iter()
                    .any(|url| !url.trim().is_empty() && hosted_upload(url.trim()).is_none())
            })
        })
        .map(|field| FieldError {
            key: field.key.clone(),
            message: "Please upload the photo again.".to_string(),
        })
        .collect()
}

/// One uploaded photo of a photo field: its address, which the form sends,
/// and a way to take it out again.
#[derive(TemplateOnce)]
#[template(path = "hosted_form/photo_item.stpl.html")]
pub(super) struct PhotoItem {
    /// The name of the field's controls.
    pub name: String,
    pub url: String,
}

/// The answer to an upload: what the field's status line says, and the
/// photos kept.
#[derive(TemplateOnce)]
#[template(path = "hosted_form/photo_uploaded.stpl.html")]
struct PhotoUploaded {
    status_id: String,
    message: &'static str,
    failed: bool,
    photos_id: String,
    /// Whether the field takes more than one photo, so that these join
    /// those it has rather than replace them.
    multiple: bool,
    photos: Vec<PhotoItem>,
}

#[derive(Debug, Deserialize)]
pub(super) struct PhotoQuery {
    /// The key of the field the photos are for.
    field: String,
}

/// POST `/-form/:form_id/photo?field=<key>`: keep the photos sent for the
/// photo field `key`, and answer with the field's status line, and out of
/// band each photo as the field shows it. A photo field taking one photo
/// keeps the first sent.
#[handler]
pub(super) async fn upload(
    Authenticated(user): Authenticated,
    Path(form_id): Path<Ulid>,
    Query(query): Query<PhotoQuery>,
    mut multipart: Multipart,
) -> Result<Response, Error> {
    let form = super::load(FormLookup::Id(form_id)).await?;

    let field = form.as_ref().and_then(|form| {
        form.fields
            .iter()
            .position(|field| field.key.as_str() == query.field)
            .map(|index| (index, &form.fields[index]))
    });

    let (form, (index, field)) = match (&form, field) {
        (Some(form), Some(field)) => (form, field),
        _ => return Ok(poem::http::StatusCode::NOT_FOUND.into()),
    };

    let FieldKind::Photo { multiple } = field.kind else {
        return Ok(poem::http::StatusCode::NOT_FOUND.into());
    };

    let answer = |message, failed, photos| PhotoUploaded {
        status_id: format!("{}-status", field_id(index)),
        message,
        failed,
        photos_id: format!("{}-photos", field_id(index)),
        multiple,
        photos,
    };

    if form.closed {
        return fragment(answer("This form is closed.", true, Vec::new()));
    }

    if form.sign_in_required && user.is_none() {
        return fragment(answer("Please log in first.", true, Vec::new()));
    }

    let uploaded_by = user.as_ref().map(|user| user.user_id);
    let mut photos = Vec::new();
    let mut refused = false;

    while let Some(part) = multipart.next_field().await? {
        if part.name() != Some("photo") || (!multiple && !photos.is_empty()) {
            continue;
        }

        let is_image = part
            .content_type()
            .is_some_and(|content_type| content_type.starts_with("image/"));
        if !is_image {
            refused = true;
            continue;
        }

        let path = crate::upload::store_untracked(part, uploaded_by).await?;
        photos.push(PhotoItem {
            name: control_name(&field.key),
            url: hosted_url(&path),
        });
    }

    let (message, failed) = match (photos.is_empty(), refused) {
        (true, true) => ("That isn't a photo. Please choose a picture.", true),
        (true, false) => ("No photo was sent. Please choose one again.", true),
        (false, true) => ("Uploaded ✓ (some files weren't pictures)", false),
        (false, false) => ("Uploaded ✓", false),
    };

    fragment(answer(message, failed, photos))
}

fn fragment(answer: PhotoUploaded) -> Result<Response, Error> {
    Ok(Response::builder()
        .content_type("text/html; charset=utf-8")
        .body(answer.render_once()?))
}

#[cfg(test)]
mod tests {
    use assert2::check;

    use super::*;

    #[test]
    fn only_this_sites_uploads_are_photos_of_it() {
        let file_id = Ulid::new();

        check!(hosted_upload(&hosted_url(&format!("/-uploaded/{file_id}"))) == Some(file_id));
        check!(hosted_upload(&format!("https://example.org/-uploaded/{file_id}")) == None);
        check!(hosted_upload(&hosted_url("/-static/img/logo.png")) == None);
    }
}
