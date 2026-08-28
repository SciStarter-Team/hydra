//! Email management handlers for stored emails, one-off emails, triggers, and queue.

use crate::component::{AdminLayout, Component};
use crate::form::{QsForm, deserialize_string_or_vec};
use crate::local_time::{TimeFormat, local_time};
use crate::middleware::authorization::Authenticated;
use poem::handler;
use poem::web::{Form, Path, Query};
use poem::{Route, get, patch, post};
use sailfish::TemplateOnce;
use serde::Deserialize;

pub fn routes() -> Route {
    Route::new()
        .at("/oneoff", get(email_oneoff).post(email_oneoff_create))
        .at("/oneoff/new", get(email_oneoff_new))
        .at(
            "/oneoff/:id",
            get(email_oneoff_edit).post(email_oneoff_update),
        )
        .at("/oneoff/:id/send", post(email_oneoff_send))
        .at(
            "/oneoff/:id/convert-to-stored",
            get(email_oneoff_convert_form).post(email_oneoff_to_stored),
        )
        .at("/stored", get(email_stored).post(email_stored_create))
        .at("/stored/new", get(email_stored_new))
        .at(
            "/stored/:id",
            get(email_stored_edit).post(email_stored_update),
        )
        .at("/stored/:id/send", post(email_stored_send))
        .at(
            "/stored/:id/convert-to-oneoff",
            post(email_stored_to_oneoff),
        )
        .at("/stored/autocomplete", get(email_stored_autocomplete))
        .at("/triggers", get(email_triggers).post(email_trigger_create))
        .at("/triggers/:id/edit", get(email_trigger_edit))
        .at(
            "/triggers/:id",
            patch(email_trigger_update).delete(email_trigger_delete),
        )
        .at("/queue", get(email_queue))
        .at("/queue-content", get(email_queue_content))
        .at("/dataset/:name/variables", get(email_dataset_variables))
        .at("/merge-fields", get(email_merge_fields))
}

// Query parameters for email pages
#[derive(Debug, Deserialize)]
pub struct EmailPageQuery {
    page: Option<u32>,
    search: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OneoffEmailPageQuery {
    page: Option<u32>,
    include_archived: Option<bool>,
}

// Recipient picker partial for email forms
#[derive(TemplateOnce)]
#[template(path = "admin/partials/recipient_picker.stpl.html")]
pub struct RecipientPickerPartial;

// Stored emails list
#[derive(Debug, TemplateOnce)]
#[template(path = "admin/email_stored.stpl.html")]
pub struct EmailStoredContent {
    pub emails: Vec<common::message::email::StoredEmail>,
    pub current_page: u32,
    pub total: u32,
    pub has_previous: bool,
    pub has_next: bool,
    pub per_page: u32,
    pub start_item: u32,
    pub end_item: u32,
}

#[handler]
async fn email_stored(
    Query(query): Query<EmailPageQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let page = query.page.unwrap_or(1);
    let per_page = 25;

    let request = common::message::email::ListStoredEmailsRequest {
        page,
        per_page,
        search: query.search,
    };

    let response: common::message::email::ListStoredEmailsResponse =
        common::service::request("emailer.stored.list", request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list stored emails: {}", e))?;

    let total_pages = (response.total + per_page - 1) / per_page;
    let has_previous = page > 1;
    let has_next = page < total_pages;
    let start_item = if response.total == 0 {
        0
    } else {
        ((page - 1) * per_page) + 1
    };
    let end_item = std::cmp::min(page * per_page, response.total);

    let content = EmailStoredContent {
        emails: response.emails,
        current_page: page,
        total: response.total,
        has_previous,
        has_next,
        per_page,
        start_item,
        end_item,
    };

    AdminLayout::new("Stored Emails", content)
        .with_breadcrumb("Stored Emails", None)
        .with_sidebar_active("email-stored")
        .into_response(user_info)
        .await
}

// Oneoff emails list
#[derive(Debug, TemplateOnce)]
#[template(path = "admin/email_oneoff.stpl.html")]
pub struct EmailOneoffContent {
    pub emails: Vec<common::message::email::OneoffEmail>,
    pub current_page: u32,
    pub total: u32,
    pub has_previous: bool,
    pub has_next: bool,
    pub per_page: u32,
    pub start_item: u32,
    pub end_item: u32,
    pub include_archived: bool,
}

#[handler]
async fn email_oneoff(
    Query(query): Query<OneoffEmailPageQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let page = query.page.unwrap_or(1);
    let per_page = 25;
    let include_archived = query.include_archived.unwrap_or(false);

    let request = common::message::email::ListOneoffEmailsRequest {
        page,
        per_page,
        include_archived,
    };

    let response: common::message::email::ListOneoffEmailsResponse =
        common::service::request("emailer.oneoff.list", request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list oneoff emails: {}", e))?;

    let total_pages = (response.total + per_page - 1) / per_page;
    let has_previous = page > 1;
    let has_next = page < total_pages;
    let start_item = if response.total == 0 {
        0
    } else {
        ((page - 1) * per_page) + 1
    };
    let end_item = std::cmp::min(page * per_page, response.total);

    let content = EmailOneoffContent {
        emails: response.emails,
        current_page: page,
        total: response.total,
        has_previous,
        has_next,
        per_page,
        start_item,
        end_item,
        include_archived,
    };

    AdminLayout::new("One-off Emails", content)
        .with_breadcrumb("One-off Emails", None)
        .with_sidebar_active("email-oneoff")
        .into_response(user_info)
        .await
}

// Stored email new page
#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/new_stored_email.stpl.html")]
pub struct NewStoredEmailPage {
    pub error_message: Option<String>,
    pub datasets: Vec<common::message::email::DatasetInfo>,
}

#[handler]
async fn email_stored_new(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Fetch available datasets
    let datasets_response =
        common::service::request::<common::message::email::ListDatasetsResponse>(
            "emailer.datasets.list",
            common::message::email::ListDatasetsRequest {},
        )
        .await
        .unwrap_or(common::message::email::ListDatasetsResponse { datasets: vec![] });

    let content = NewStoredEmailPage {
        error_message: None,
        datasets: datasets_response.datasets,
    };
    AdminLayout::new("Create Stored Email", content)
        .with_breadcrumb("Stored Emails", Some("/-admin/email/stored"))
        .with_breadcrumb("Create", None)
        .with_sidebar_active("email-stored")
        .with_quill()
        .into_response(user_info)
        .await
}

// Stored email create handler
#[derive(Debug, Deserialize)]
pub struct CreateStoredEmailForm {
    slug: String,
    subject: String,
    body_delta: String,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    datasets: Vec<String>,
    /// Absent from the form body when the checkbox is unchecked.
    #[serde(default)]
    reply_to_noreply: bool,
}

#[handler]
async fn email_stored_create(
    Form(form): Form<CreateStoredEmailForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::email::CreateStoredEmailRequest {
        slug: form.slug,
        subject: form.subject,
        body_delta: form.body_delta,
        datasets: form.datasets,
        reply_to_noreply: form.reply_to_noreply,
    };

    match common::service::request::<common::message::email::CreateStoredEmailResponse>(
        "emailer.stored.create",
        request,
    )
    .await
    {
        Ok(_response) => Ok(poem::Response::builder()
            .status(poem::http::StatusCode::SEE_OTHER)
            .header("Location", "/-admin/email/stored")
            .finish()),
        Err(e) => {
            // Re-fetch datasets for error display
            let datasets_response =
                common::service::request::<common::message::email::ListDatasetsResponse>(
                    "emailer.datasets.list",
                    common::message::email::ListDatasetsRequest {},
                )
                .await
                .unwrap_or(common::message::email::ListDatasetsResponse { datasets: vec![] });

            let content = NewStoredEmailPage {
                error_message: Some(format!("Failed to create stored email: {}", e)),
                datasets: datasets_response.datasets,
            };
            AdminLayout::new("Create Stored Email", content)
                .with_breadcrumb("Stored Emails", Some("/-admin/email/stored"))
                .with_breadcrumb("Create", None)
                .with_sidebar_active("email-stored")
                .into_response(user_info)
                .await
        }
    }
}

// Stored email edit page
#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/edit_stored_email.stpl.html")]
pub struct EditStoredEmailPage {
    pub stored_email_id: ulid::Ulid,
    pub slug: String,
    pub subject: String,
    pub body_delta: String,
    pub selected_datasets: Vec<String>,
    pub reply_to_noreply: bool,
    pub error_message: Option<String>,
    pub available_datasets: Vec<common::message::email::DatasetInfo>,
}

#[handler]
async fn email_stored_edit(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let stored_email_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid stored email ID format"))?;

    let request = common::message::email::GetStoredEmailRequest { stored_email_id };

    // Fetch datasets
    let datasets_response =
        common::service::request::<common::message::email::ListDatasetsResponse>(
            "emailer.datasets.list",
            common::message::email::ListDatasetsRequest {},
        )
        .await
        .unwrap_or(common::message::email::ListDatasetsResponse { datasets: vec![] });

    match common::service::request::<common::message::email::GetStoredEmailResponse>(
        "emailer.stored.get",
        request,
    )
    .await
    {
        Ok(response) => {
            let email = response.email;
            let content = EditStoredEmailPage {
                stored_email_id: email.stored_email_id,
                slug: email.slug,
                subject: email.subject,
                body_delta: email.body_delta,
                selected_datasets: email.datasets,
                reply_to_noreply: email.reply_to_noreply,
                error_message: None,
                available_datasets: datasets_response.datasets,
            };
            AdminLayout::new("Edit Stored Email", content)
                .with_breadcrumb("Stored Emails", Some("/-admin/email/stored"))
                .with_breadcrumb("Edit", None)
                .with_sidebar_active("email-stored")
                .with_quill()
                .into_response(user_info)
                .await
        }
        Err(e) => Err(anyhow::anyhow!("Failed to load stored email: {}", e)),
    }
}

// Stored email update handler
#[derive(Debug, Deserialize)]
pub struct UpdateStoredEmailForm {
    action: String,
    slug: Option<String>,
    subject: Option<String>,
    body_delta: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    datasets: Vec<String>,
    /// Absent from the form body when the checkbox is unchecked.
    #[serde(default)]
    reply_to_noreply: bool,
}

#[handler]
async fn email_stored_update(
    Path(id): Path<String>,
    Form(form): Form<UpdateStoredEmailForm>,
    Authenticated(_user_info): Authenticated,
    req: &poem::Request,
) -> Result<poem::Response, anyhow::Error> {
    let stored_email_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid stored email ID format"))?;

    match form.action.as_str() {
        "delete" => {
            let request = common::message::email::DeleteStoredEmailRequest { stored_email_id };
            common::service::request::<common::message::email::DeleteStoredEmailResponse>(
                "emailer.stored.delete",
                request,
            )
            .await?;
        }
        "save" => {
            let request = common::message::email::UpdateStoredEmailRequest {
                stored_email_id,
                slug: form.slug,
                subject: form.subject,
                body_delta: form.body_delta,
                datasets: if form.datasets.is_empty() {
                    None
                } else {
                    Some(form.datasets)
                },
                // Always sent: the edit form posts the full checkbox state, so
                // an absent field means "unchecked", not "leave unchanged".
                reply_to_noreply: Some(form.reply_to_noreply),
            };
            common::service::request::<common::message::email::UpdateStoredEmailResponse>(
                "emailer.stored.update",
                request,
            )
            .await?;
        }
        _ => return Err(anyhow::anyhow!("Invalid action")),
    }

    // If this is an HTMX request, just return 200 OK
    if req.headers().get("HX-Request").is_some() {
        Ok(poem::Response::builder()
            .status(poem::http::StatusCode::OK)
            .finish())
    } else {
        // Otherwise redirect to the list page
        Ok(poem::Response::builder()
            .status(poem::http::StatusCode::SEE_OTHER)
            .header("Location", "/-admin/email/stored")
            .finish())
    }
}

#[derive(Debug, Deserialize)]
struct SendOneoffEmailForm {
    #[serde(default)]
    recipient_types: Vec<String>,
    #[serde(default)]
    recipient_values: Vec<String>,
}

#[handler]
async fn email_stored_send(
    Path(id): Path<String>,
    QsForm(form): QsForm<SendOneoffEmailForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let stored_email_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid stored email ID format"))?;

    // Get the email template
    let email_response: common::message::email::GetStoredEmailResponse = common::service::request(
        "emailer.stored.get",
        common::message::email::GetStoredEmailRequest { stored_email_id },
    )
    .await?;

    // Build recipients from parallel arrays
    if form.recipient_types.len() != form.recipient_values.len() {
        return Err(anyhow::anyhow!("Mismatched recipient data"));
    }

    if form.recipient_types.is_empty() {
        return Err(anyhow::anyhow!("No recipients specified"));
    }

    let mut recipients = Vec::new();
    for (recipient_type, recipient_value) in form
        .recipient_types
        .iter()
        .zip(form.recipient_values.iter())
    {
        let recipient = match recipient_type.as_str() {
            "Email" => common::message::email::EmailRecipient::Email(recipient_value.clone()),
            "User" => {
                let user_id = recipient_value
                    .parse::<ulid::Ulid>()
                    .map_err(|_| anyhow::anyhow!("Invalid user ID"))?;
                common::message::email::EmailRecipient::UserId(user_id)
            }
            "UserSet" => common::message::email::EmailRecipient::UserSet {
                slug: recipient_value.clone(),
            },
            "AddressSet" => common::message::email::EmailRecipient::AddressSet {
                slug: recipient_value.clone(),
            },
            _ => return Err(anyhow::anyhow!("Invalid recipient type")),
        };
        recipients.push(recipient);
    }

    // Convert delta to HTML
    let template_html = common::quill::delta_to_html(&email_response.email.body_delta)?;

    // Send the email
    let _response: common::message::email::SendEmailResponse = common::service::request(
        "emailer.send",
        common::message::email::SendEmailRequest {
            template_html,
            subject: email_response.email.subject,
            recipients,
            data_sets: email_response.email.datasets,
            additional_data: None,
            reply_to: common::message::email::noreply_reply_to(
                email_response.email.reply_to_noreply,
            ),
        },
    )
    .await?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", format!("/-admin/email/stored/{}", id))
        .finish())
}

// Oneoff email new page
#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/new_oneoff_email.stpl.html")]
pub struct NewOneoffEmailPage {
    pub error_message: Option<String>,
    pub datasets: Vec<common::message::email::DatasetInfo>,
}

#[handler]
async fn email_oneoff_new(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Fetch available datasets
    let datasets_response =
        common::service::request::<common::message::email::ListDatasetsResponse>(
            "emailer.datasets.list",
            common::message::email::ListDatasetsRequest {},
        )
        .await
        .unwrap_or(common::message::email::ListDatasetsResponse { datasets: vec![] });

    let content = NewOneoffEmailPage {
        error_message: None,
        datasets: datasets_response.datasets,
    };
    AdminLayout::new("Create One-off Email", content)
        .with_breadcrumb("One-off Emails", Some("/-admin/email/oneoff"))
        .with_breadcrumb("Create", None)
        .with_sidebar_active("email-oneoff")
        .with_quill()
        .into_response(user_info)
        .await
}

// Oneoff email create handler
#[derive(Debug, Deserialize)]
pub struct CreateOneoffEmailForm {
    subject: String,
    body_delta: String,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    datasets: Vec<String>,
    /// Absent from the form body when the checkbox is unchecked.
    #[serde(default)]
    reply_to_noreply: bool,
}

#[handler]
async fn email_oneoff_create(
    Form(form): Form<CreateOneoffEmailForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::email::CreateOneoffEmailRequest {
        subject: form.subject,
        body_delta: form.body_delta,
        datasets: form.datasets,
        reply_to_noreply: form.reply_to_noreply,
    };

    match common::service::request::<common::message::email::CreateOneoffEmailResponse>(
        "emailer.oneoff.create",
        request,
    )
    .await
    {
        Ok(_response) => Ok(poem::Response::builder()
            .status(poem::http::StatusCode::SEE_OTHER)
            .header("Location", "/-admin/email/oneoff")
            .finish()),
        Err(e) => {
            // Re-fetch datasets for error display
            let datasets_response =
                common::service::request::<common::message::email::ListDatasetsResponse>(
                    "emailer.datasets.list",
                    common::message::email::ListDatasetsRequest {},
                )
                .await
                .unwrap_or(common::message::email::ListDatasetsResponse { datasets: vec![] });

            let content = NewOneoffEmailPage {
                error_message: Some(format!("Failed to create one-off email: {}", e)),
                datasets: datasets_response.datasets,
            };
            AdminLayout::new("Create One-off Email", content)
                .with_breadcrumb("One-off Emails", Some("/-admin/email/oneoff"))
                .with_breadcrumb("Create", None)
                .with_sidebar_active("email-oneoff")
                .into_response(user_info)
                .await
        }
    }
}

// Oneoff email edit page
#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/edit_oneoff_email.stpl.html")]
pub struct EditOneoffEmailPage {
    pub oneoff_email_id: ulid::Ulid,
    pub subject: String,
    pub body_delta: String,
    pub archived: bool,
    pub selected_datasets: Vec<String>,
    pub reply_to_noreply: bool,
    pub error_message: Option<String>,
    pub available_datasets: Vec<common::message::email::DatasetInfo>,
    pub legacy_slug: Option<String>,
}

#[handler]
async fn email_oneoff_edit(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let oneoff_email_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid oneoff email ID format"))?;

    let request = common::message::email::GetOneoffEmailRequest { oneoff_email_id };

    // Fetch datasets
    let datasets_response =
        common::service::request::<common::message::email::ListDatasetsResponse>(
            "emailer.datasets.list",
            common::message::email::ListDatasetsRequest {},
        )
        .await
        .unwrap_or(common::message::email::ListDatasetsResponse { datasets: vec![] });

    match common::service::request::<common::message::email::GetOneoffEmailResponse>(
        "emailer.oneoff.get",
        request,
    )
    .await
    {
        Ok(response) => {
            let email = response.email;
            let content = EditOneoffEmailPage {
                oneoff_email_id: email.oneoff_email_id,
                subject: email.subject,
                body_delta: email.body_delta,
                archived: email.archived,
                selected_datasets: email.datasets,
                reply_to_noreply: email.reply_to_noreply,
                error_message: None,
                available_datasets: datasets_response.datasets,
                legacy_slug: email.legacy_slug,
            };
            AdminLayout::new("Edit One-off Email", content)
                .with_breadcrumb("One-off Emails", Some("/-admin/email/oneoff"))
                .with_breadcrumb("Edit", None)
                .with_sidebar_active("email-oneoff")
                .with_quill()
                .into_response(user_info)
                .await
        }
        Err(e) => Err(anyhow::anyhow!("Failed to load one-off email: {}", e)),
    }
}

// Oneoff email update handler
#[derive(Debug, Deserialize)]
pub struct UpdateOneoffEmailForm {
    action: String,
    subject: Option<String>,
    body_delta: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    datasets: Vec<String>,
    /// Absent from the form body when the checkbox is unchecked.
    #[serde(default)]
    reply_to_noreply: bool,
}

#[handler]
async fn email_oneoff_update(
    Path(id): Path<String>,
    Form(form): Form<UpdateOneoffEmailForm>,
    Authenticated(_user_info): Authenticated,
    req: &poem::Request,
) -> Result<poem::Response, anyhow::Error> {
    let oneoff_email_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid oneoff email ID format"))?;

    match form.action.as_str() {
        "delete" => {
            let request = common::message::email::DeleteOneoffEmailRequest { oneoff_email_id };
            common::service::request::<common::message::email::DeleteOneoffEmailResponse>(
                "emailer.oneoff.delete",
                request,
            )
            .await?;
        }
        "archive" => {
            let request = common::message::email::ArchiveOneoffEmailRequest {
                oneoff_email_id,
                archived: true,
            };
            common::service::request::<common::message::email::ArchiveOneoffEmailResponse>(
                "emailer.oneoff.archive",
                request,
            )
            .await?;
        }
        "unarchive" => {
            let request = common::message::email::ArchiveOneoffEmailRequest {
                oneoff_email_id,
                archived: false,
            };
            common::service::request::<common::message::email::ArchiveOneoffEmailResponse>(
                "emailer.oneoff.archive",
                request,
            )
            .await?;
        }
        "save" => {
            let request = common::message::email::UpdateOneoffEmailRequest {
                oneoff_email_id,
                subject: form.subject,
                body_delta: form.body_delta,
                datasets: if form.datasets.is_empty() {
                    None
                } else {
                    Some(form.datasets)
                },
                // Always sent: the edit form posts the full checkbox state, so
                // an absent field means "unchecked", not "leave unchanged".
                reply_to_noreply: Some(form.reply_to_noreply),
            };
            common::service::request::<common::message::email::UpdateOneoffEmailResponse>(
                "emailer.oneoff.update",
                request,
            )
            .await?;
        }
        _ => return Err(anyhow::anyhow!("Invalid action")),
    }

    // If this is an HTMX request, just return 200 OK
    if req.headers().get("HX-Request").is_some() {
        Ok(poem::Response::builder()
            .status(poem::http::StatusCode::OK)
            .finish())
    } else {
        // Otherwise redirect to the list page
        Ok(poem::Response::builder()
            .status(poem::http::StatusCode::SEE_OTHER)
            .header("Location", "/-admin/email/oneoff")
            .finish())
    }
}

#[handler]
async fn email_oneoff_send(
    Path(id): Path<String>,
    QsForm(form): QsForm<SendOneoffEmailForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let oneoff_email_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid oneoff email ID format"))?;

    // Get the email template
    let email_response: common::message::email::GetOneoffEmailResponse = common::service::request(
        "emailer.oneoff.get",
        common::message::email::GetOneoffEmailRequest { oneoff_email_id },
    )
    .await?;

    // Build recipients from parallel arrays
    if form.recipient_types.len() != form.recipient_values.len() {
        return Err(anyhow::anyhow!("Mismatched recipient data"));
    }

    if form.recipient_types.is_empty() {
        return Err(anyhow::anyhow!("No recipients specified"));
    }

    let mut recipients = Vec::new();
    for (recipient_type, recipient_value) in form
        .recipient_types
        .iter()
        .zip(form.recipient_values.iter())
    {
        let recipient = match recipient_type.as_str() {
            "Email" => common::message::email::EmailRecipient::Email(recipient_value.clone()),
            "User" => {
                let user_id = recipient_value
                    .parse::<ulid::Ulid>()
                    .map_err(|_| anyhow::anyhow!("Invalid user ID"))?;
                common::message::email::EmailRecipient::UserId(user_id)
            }
            "UserSet" => common::message::email::EmailRecipient::UserSet {
                slug: recipient_value.clone(),
            },
            "AddressSet" => common::message::email::EmailRecipient::AddressSet {
                slug: recipient_value.clone(),
            },
            _ => return Err(anyhow::anyhow!("Invalid recipient type")),
        };
        recipients.push(recipient);
    }

    // Convert delta to HTML
    let template_html = common::quill::delta_to_html(&email_response.email.body_delta)?;

    // Send the email
    let _response: common::message::email::SendEmailResponse = common::service::request(
        "emailer.send",
        common::message::email::SendEmailRequest {
            template_html,
            subject: email_response.email.subject,
            recipients,
            data_sets: email_response.email.datasets,
            additional_data: None,
            reply_to: common::message::email::noreply_reply_to(
                email_response.email.reply_to_noreply,
            ),
        },
    )
    .await?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", format!("/-admin/email/oneoff/{}", id))
        .finish())
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/email_triggers.stpl.html")]
pub struct EmailTriggersContent {
    pub triggers: Vec<common::message::email::TriggerInfo>,
    pub available_trigger_events: Vec<(String, String, String)>, // (name, description, category)
    pub stored_emails: std::collections::HashMap<ulid::Ulid, (String, String)>, // id -> (slug, subject)
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/trigger_row.stpl.html")]
pub struct TriggerRow {
    pub trigger: common::message::email::TriggerInfo,
    pub stored_email: Option<(String, String)>, // (slug, subject)
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/trigger_list.stpl.html")]
pub struct TriggerList {
    pub triggers: Vec<common::message::email::TriggerInfo>,
    pub stored_emails: std::collections::HashMap<ulid::Ulid, (String, String)>, // id -> (slug, subject)
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/modals/edit_trigger.stpl.html")]
pub struct EditTriggerModal {
    pub trigger: common::message::email::TriggerInfo,
    pub available_trigger_events: Vec<(String, String, String)>, // (name, description, category)
    pub stored_email: Option<(String, String)>,                  // (slug, subject)
    pub current_recipients: Vec<(String, String)>,               // (form_value, display_label)
}

#[handler]
async fn email_triggers(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use common::message::email::{
        GetStoredEmailRequest, GetStoredEmailResponse, ListTriggersRequest, ListTriggersResponse,
    };
    use common::service::{TriggerEvent, request};
    use std::collections::HashMap;
    use strum::IntoEnumIterator;

    // Get all configured triggers
    let triggers_response: ListTriggersResponse = request(
        "emailer.triggers.list",
        ListTriggersRequest { active_only: false },
    )
    .await?;

    // Get all available trigger events from the enum
    let available_trigger_events: Vec<(String, String, String)> = TriggerEvent::iter()
        .map(|trigger| {
            (
                trigger.as_ref().to_string(),
                trigger.description().to_string(),
                trigger.category().to_string(),
            )
        })
        .collect();

    // Fetch stored email info for triggers (only the ones referenced)
    let mut stored_emails = HashMap::new();
    for trigger in &triggers_response.triggers {
        if !stored_emails.contains_key(&trigger.stored_email_id) {
            if let Ok(email_response) = request::<GetStoredEmailResponse>(
                "emailer.stored.get",
                GetStoredEmailRequest {
                    stored_email_id: trigger.stored_email_id,
                },
            )
            .await
            {
                stored_emails.insert(
                    email_response.email.stored_email_id,
                    (email_response.email.slug, email_response.email.subject),
                );
            }
        }
    }

    let content = EmailTriggersContent {
        triggers: triggers_response.triggers,
        available_trigger_events,
        stored_emails,
    };

    AdminLayout::new("Email Triggers", content)
        .with_breadcrumb("Email Triggers", None)
        .with_sidebar_active("email-triggers")
        .into_response(user_info)
        .await
}

// Stored email autocomplete for trigger forms
#[derive(Debug, Deserialize)]
pub struct StoredEmailAutocompleteQuery {
    q: Option<String>,
}

#[handler]
async fn email_stored_autocomplete(
    Query(query): Query<StoredEmailAutocompleteQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use common::message::email::{ListStoredEmailsRequest, ListStoredEmailsResponse};
    use common::service::request;

    let search = query.q.filter(|s| !s.is_empty());

    let response: ListStoredEmailsResponse = request(
        "emailer.stored.list",
        ListStoredEmailsRequest {
            page: 1,
            per_page: 20,
            search,
        },
    )
    .await?;

    if response.emails.is_empty() {
        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(r#"<div class="hidden"></div>"#));
    }

    let results = response
        .emails
        .iter()
        .map(|email| {
            format!(
                r#"<div class="cursor-pointer px-3 py-2 hover:bg-gray-100 border-b border-gray-100 last:border-b-0"
                     hx-on:click="
                         let widget = this.closest('[data-widget=stored-email-autocomplete]');
                         htmx.find(widget, '[data-part=stored-email-id]').value = '{}';
                         htmx.find(widget, '[data-part=stored-email-search]').value = '{} - {}';
                         htmx.find(widget, '[data-part=results]').classList.add('hidden');
                     ">
                    <div class="font-medium text-gray-900">{}</div>
                    <div class="text-sm text-gray-500">{}</div>
                </div>"#,
                email.stored_email_id,
                email.slug,
                email.subject,
                email.slug,
                email.subject
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let html = format!(
        r#"<div data-part="results" class="absolute z-10 w-full mt-1 bg-white border border-gray-300 rounded-md shadow-lg max-h-60 overflow-auto">
            {}
        </div>"#,
        results
    );

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

/// Convert a form recipient value to its JSON representation.
/// Form values use a prefix convention:
/// - "triggering_user" → "triggering_user"
/// - "address_set:slug" → {"address_set": "slug"}
/// - "user_set:slug" → {"user_set": "slug"}
/// - "email:addr" → {"email": "addr"}
fn recipient_form_value_to_json(value: &str) -> serde_json::Value {
    if value == "triggering_user" {
        serde_json::Value::String("triggering_user".to_string())
    } else if let Some(slug) = value.strip_prefix("address_set:") {
        serde_json::json!({"address_set": slug})
    } else if let Some(slug) = value.strip_prefix("user_set:") {
        serde_json::json!({"user_set": slug})
    } else if let Some(email) = value.strip_prefix("email:") {
        serde_json::json!({"email": email})
    } else {
        serde_json::Value::String(value.to_string())
    }
}

/// Convert recipients JSON back to form values for display.
fn recipients_json_to_form_values(json: &Option<String>) -> Vec<(String, String)> {
    let Some(json) = json else {
        return vec![("triggering_user".to_string(), "Triggering User".to_string())];
    };

    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        return vec![("triggering_user".to_string(), "Triggering User".to_string())];
    };

    entries
        .into_iter()
        .filter_map(|entry| match entry {
            serde_json::Value::String(s) if s == "triggering_user" => {
                Some(("triggering_user".to_string(), "Triggering User".to_string()))
            }
            serde_json::Value::Object(ref map) => {
                if let Some(slug) = map.get("address_set").and_then(|v| v.as_str()) {
                    Some((
                        format!("address_set:{}", slug),
                        format!("Address Set: {}", slug),
                    ))
                } else if let Some(slug) = map.get("user_set").and_then(|v| v.as_str()) {
                    Some((format!("user_set:{}", slug), format!("User Set: {}", slug)))
                } else if let Some(email) = map.get("email").and_then(|v| v.as_str()) {
                    Some((format!("email:{}", email), format!("Email: {}", email)))
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect()
}

// Create email trigger
#[derive(Debug, Deserialize)]
pub struct CreateTriggerForm {
    trigger_name: String,
    stored_email_id: String,
    action_type: String,
    delay_seconds: Option<String>,
    cancellation_trigger_name: Option<String>,
    #[serde(default)]
    recipients: Vec<String>,
}

#[handler]
async fn email_trigger_create(
    Form(form): Form<CreateTriggerForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;
    use common::message::email::{
        CreateTriggerRequest, CreateTriggerResponse, GetStoredEmailRequest, GetStoredEmailResponse,
        ListTriggersRequest, ListTriggersResponse, TriggerActionType,
    };
    use common::service::request;
    use std::collections::HashMap;

    let stored_email_id = form
        .stored_email_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid stored email ID"))?;

    let action_type = TriggerActionType::from_str(&form.action_type)
        .ok_or_else(|| anyhow::anyhow!("Invalid action type"))?;

    let delay_seconds = if action_type == TriggerActionType::Delayed {
        Some(
            form.delay_seconds
                .and_then(|s| s.parse::<i64>().ok())
                .ok_or_else(|| anyhow::anyhow!("Delay seconds required for delayed triggers"))?,
        )
    } else {
        None
    };

    let cancellation_trigger_name = form.cancellation_trigger_name.filter(|s| !s.is_empty());

    let recipients = if form.recipients.is_empty() {
        None
    } else {
        let entries: Vec<serde_json::Value> = form
            .recipients
            .iter()
            .map(|r| recipient_form_value_to_json(r))
            .collect();
        Some(serde_json::to_string(&entries).unwrap_or_default())
    };

    let create_request = CreateTriggerRequest {
        trigger_name: form.trigger_name,
        stored_email_id,
        action_type,
        delay_seconds,
        cancellation_trigger_name,
        recipients,
    };

    let _response: CreateTriggerResponse =
        request("emailer.triggers.create", create_request).await?;

    // Fetch updated triggers list
    let triggers_response: ListTriggersResponse = request(
        "emailer.triggers.list",
        ListTriggersRequest { active_only: false },
    )
    .await?;

    // Fetch stored email info for triggers
    let mut stored_emails = HashMap::new();
    for trigger in &triggers_response.triggers {
        if !stored_emails.contains_key(&trigger.stored_email_id) {
            if let Ok(email_response) = request::<GetStoredEmailResponse>(
                "emailer.stored.get",
                GetStoredEmailRequest {
                    stored_email_id: trigger.stored_email_id,
                },
            )
            .await
            {
                stored_emails.insert(
                    trigger.stored_email_id,
                    (email_response.email.slug, email_response.email.subject),
                );
            }
        }
    }

    // Render the updated list
    let list = TriggerList {
        triggers: triggers_response.triggers,
        stored_emails,
    };

    common::service::log_activity(None, "email-trigger-create", user_info.user_id, None, None)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(list.render_once()?))
}

// Get edit trigger form
#[handler]
async fn email_trigger_edit(
    Path(id): Path<String>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use common::message::email::{
        GetStoredEmailRequest, GetStoredEmailResponse, GetTriggerRequest, GetTriggerResponse,
    };
    use common::service::{TriggerEvent, request};
    use strum::IntoEnumIterator;

    let trigger_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid trigger ID"))?;

    // Fetch the trigger
    let trigger_response: GetTriggerResponse =
        request("emailer.triggers.get", GetTriggerRequest { trigger_id }).await?;

    let trigger = trigger_response.trigger;

    // Get all available trigger events
    let available_trigger_events: Vec<(String, String, String)> = TriggerEvent::iter()
        .map(|trigger| {
            (
                trigger.as_ref().to_string(),
                trigger.description().to_string(),
                trigger.category().to_string(),
            )
        })
        .collect();

    // Fetch stored email info
    let stored_email = if let Ok(email_response) = request::<GetStoredEmailResponse>(
        "emailer.stored.get",
        GetStoredEmailRequest {
            stored_email_id: trigger.stored_email_id,
        },
    )
    .await
    {
        Some((email_response.email.slug, email_response.email.subject))
    } else {
        None
    };

    let current_recipients = recipients_json_to_form_values(&trigger.recipients);

    // Render the modal
    let modal = EditTriggerModal {
        trigger,
        available_trigger_events,
        stored_email,
        current_recipients,
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(modal.render_once()?))
}

// Update email trigger
#[derive(Debug, Deserialize)]
pub struct UpdateTriggerForm {
    stored_email_id: Option<String>,
    action_type: Option<String>,
    delay_seconds: Option<String>,
    cancellation_trigger_name: Option<String>,
    #[serde(default)]
    recipients: Option<Vec<String>>,
    active: Option<String>,
}

#[handler]
async fn email_trigger_update(
    Path(id): Path<String>,
    Form(form): Form<UpdateTriggerForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use common::message::email::{
        GetStoredEmailRequest, GetStoredEmailResponse, GetTriggerRequest, GetTriggerResponse,
        TriggerActionType, UpdateTriggerRequest, UpdateTriggerResponse,
    };
    use common::service::request;

    let trigger_id = match id.parse::<ulid::Ulid>() {
        Ok(id) => id,
        Err(_) => {
            return Ok(poem::Response::builder()
                .content_type("text/html")
                .body(r#"<div id="edit-trigger-error" class="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded mb-4" hx-swap-oob="true">Error: Invalid trigger ID</div>"#.to_string()));
        }
    };

    // If stored_email_id is provided (even as empty string), validate it
    let stored_email_id = if let Some(ref email_id_str) = form.stored_email_id {
        if email_id_str.is_empty() {
            return Ok(poem::Response::builder()
                .content_type("text/html")
                .body(r#"<div id="edit-trigger-error" class="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded mb-4" hx-swap-oob="true">Error: Please select an email template</div>"#.to_string()));
        }
        match email_id_str.parse::<ulid::Ulid>() {
            Ok(id) => Some(id),
            Err(_) => {
                return Ok(poem::Response::builder()
                    .content_type("text/html")
                    .body(r#"<div id="edit-trigger-error" class="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded mb-4" hx-swap-oob="true">Error: Invalid stored email ID</div>"#.to_string()));
            }
        }
    } else {
        None
    };

    let action_type = form
        .action_type
        .and_then(|s| TriggerActionType::from_str(&s));

    let delay_seconds = form.delay_seconds.and_then(|s| s.parse::<i64>().ok());

    let cancellation_trigger_name = form.cancellation_trigger_name.filter(|s| !s.is_empty());

    let active = form.active.map(|s| s == "true" || s == "1");

    let recipients = form.recipients.map(|r| {
        let entries: Vec<serde_json::Value> =
            r.iter().map(|v| recipient_form_value_to_json(v)).collect();
        serde_json::to_string(&entries).unwrap_or_default()
    });

    let update_request = UpdateTriggerRequest {
        trigger_id,
        stored_email_id,
        action_type,
        delay_seconds,
        cancellation_trigger_name,
        recipients,
        active,
    };

    let update_result: Result<UpdateTriggerResponse, _> =
        request("emailer.triggers.update", update_request).await;

    if let Err(err) = update_result {
        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(format!(r#"<div id="edit-trigger-error" class="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded mb-4" hx-swap-oob="true">Error: {}</div>"#, err)));
    }

    // Fetch the updated trigger to return the row HTML
    let trigger_response: GetTriggerResponse =
        request("emailer.triggers.get", GetTriggerRequest { trigger_id }).await?;

    let trigger = trigger_response.trigger;

    // Fetch stored email info
    let stored_email = if let Ok(email_response) = request::<GetStoredEmailResponse>(
        "emailer.stored.get",
        GetStoredEmailRequest {
            stored_email_id: trigger.stored_email_id,
        },
    )
    .await
    {
        Some((email_response.email.slug, email_response.email.subject))
    } else {
        None
    };

    // Render the updated table row using template
    let row = TriggerRow {
        trigger: trigger.clone(),
        stored_email,
    };

    let row_html = row.render_once()?;
    let row_with_oob = row_html.replace(
        &format!(r#"id="trigger-row-{}""#, trigger.trigger_id),
        &format!(
            r#"id="trigger-row-{}" hx-swap-oob="true""#,
            trigger.trigger_id
        ),
    );

    let body = format!(
        r#"{}<div id="edit-trigger-dialog" hx-swap-oob="delete"></div>"#,
        row_with_oob
    );

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(body))
}

// Delete email trigger
#[handler]
async fn email_trigger_delete(
    Path(id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    use common::message::email::{DeleteTriggerRequest, DeleteTriggerResponse};
    use common::service::request;

    let trigger_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid trigger ID"))?;

    let _response: DeleteTriggerResponse = request(
        "emailer.triggers.delete",
        DeleteTriggerRequest { trigger_id },
    )
    .await?;

    common::service::log_activity(
        None,
        "email-trigger-delete",
        user_info.user_id,
        Some(trigger_id),
        None,
    )
    .await?;

    // Return empty response - HTMX will delete the row with hx-swap="delete"
    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .body(""))
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/email_queue.stpl.html")]
pub struct EmailQueueContent {
    queue_status: common::message::email::QueueStatusResponse,
    sendings: Vec<common::message::email::SendingMetrics>,
    total: u32,
}

#[handler]
async fn email_queue(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use common::message::email::{ListSendingsRequest, ListSendingsResponse, QueueStatusResponse};
    use common::service::request;

    // Get queue status
    let queue_status: QueueStatusResponse = request("emailer.analytics.queue_status", ()).await?;

    // Get recent mass sendings (2+ targets, first page)
    let sendings_response: ListSendingsResponse = request(
        "emailer.analytics.list_sendings",
        ListSendingsRequest {
            page: 1,
            per_page: 20,
            min_queued: Some(2),
            created_from: None,
            created_to: None,
        },
    )
    .await?;

    let content = EmailQueueContent {
        queue_status,
        sendings: sendings_response.sendings,
        total: sendings_response.total,
    };

    AdminLayout::new("Email Queue", content)
        .with_breadcrumb("Email Queue", None)
        .with_sidebar_active("email-queue")
        .into_response(user_info)
        .await
}

#[handler]
async fn email_queue_content(
    Authenticated(_user_info): Authenticated,
) -> Result<impl poem::IntoResponse, anyhow::Error> {
    use common::message::email::{ListSendingsRequest, ListSendingsResponse, QueueStatusResponse};
    use common::service::request;

    // Get queue status
    let queue_status: QueueStatusResponse = request("emailer.analytics.queue_status", ()).await?;

    // Get recent mass sendings (2+ targets, first page)
    let sendings_response: ListSendingsResponse = request(
        "emailer.analytics.list_sendings",
        ListSendingsRequest {
            page: 1,
            per_page: 20,
            min_queued: Some(2),
            created_from: None,
            created_to: None,
        },
    )
    .await?;

    let content = EmailQueueContent {
        queue_status,
        sendings: sendings_response.sendings,
        total: sendings_response.total,
    }
    .render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(content))
}

// HTMX endpoint to fetch merge fields HTML fragment for selected datasets
#[derive(Debug, Deserialize)]
pub struct MergeFieldsQuery {
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    datasets: Vec<String>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/merge_fields.stpl.html")]
pub struct MergeFieldsPartial {
    pub fields: Vec<common::message::email::VariableInfo>,
}

#[handler]
async fn email_merge_fields(
    crate::form::QsQuery(query): crate::form::QsQuery<MergeFieldsQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let mut all_fields = Vec::new();

    // Fetch variables for each selected dataset
    for dataset_name in &query.datasets {
        let request = common::message::email::GetDatasetVariablesRequest {
            dataset_name: dataset_name.clone(),
        };

        if let Ok(response) = common::service::request::<
            common::message::email::GetDatasetVariablesResponse,
        >("emailer.datasets.variables", request)
        .await
        {
            all_fields.extend(response.variables);
        }
    }

    let content = MergeFieldsPartial { fields: all_fields };
    let html = content.render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

// Convert one-off email to stored: form fragment
#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/convert_to_stored_form.stpl.html")]
pub struct ConvertToStoredForm {
    pub oneoff_email_id: ulid::Ulid,
    pub suggested_slug: String,
}

#[handler]
async fn email_oneoff_convert_form(
    Path(id): Path<String>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let oneoff_email_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid oneoff email ID format"))?;

    // Fetch the email to get legacy_slug for pre-fill
    let response: common::message::email::GetOneoffEmailResponse = common::service::request(
        "emailer.oneoff.get",
        common::message::email::GetOneoffEmailRequest { oneoff_email_id },
    )
    .await?;

    let suggested_slug = response.email.legacy_slug.unwrap_or_default();

    let content = ConvertToStoredForm {
        oneoff_email_id,
        suggested_slug,
    };
    let html = content.render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

// Convert one-off email to stored: POST handler
#[derive(Debug, Deserialize)]
pub struct ConvertToStoredEmailForm {
    slug: String,
}

#[handler]
async fn email_oneoff_to_stored(
    Path(id): Path<String>,
    Form(form): Form<ConvertToStoredEmailForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let oneoff_email_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid oneoff email ID format"))?;

    // Fetch the one-off email
    let response: common::message::email::GetOneoffEmailResponse = common::service::request(
        "emailer.oneoff.get",
        common::message::email::GetOneoffEmailRequest { oneoff_email_id },
    )
    .await?;

    let email = response.email;

    // Create new stored email
    let create_response: common::message::email::CreateStoredEmailResponse =
        common::service::request(
            "emailer.stored.create",
            common::message::email::CreateStoredEmailRequest {
                slug: form.slug,
                subject: email.subject,
                body_delta: email.body_delta,
                datasets: email.datasets,
                reply_to_noreply: email.reply_to_noreply,
            },
        )
        .await?;

    // Delete the one-off email
    let _: common::message::email::DeleteOneoffEmailResponse = common::service::request(
        "emailer.oneoff.delete",
        common::message::email::DeleteOneoffEmailRequest { oneoff_email_id },
    )
    .await?;

    let redirect_url = format!("/-admin/email/stored/{}", create_response.stored_email_id);

    Ok(poem::Response::builder()
        .header("HX-Redirect", &redirect_url)
        .body(""))
}

// Convert stored email to one-off: POST handler
#[handler]
async fn email_stored_to_oneoff(
    Path(id): Path<String>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let stored_email_id = id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid stored email ID format"))?;

    // Fetch the stored email
    let response: common::message::email::GetStoredEmailResponse = common::service::request(
        "emailer.stored.get",
        common::message::email::GetStoredEmailRequest { stored_email_id },
    )
    .await?;

    let email = response.email;

    // Create new one-off email
    let create_response: common::message::email::CreateOneoffEmailResponse =
        common::service::request(
            "emailer.oneoff.create",
            common::message::email::CreateOneoffEmailRequest {
                subject: email.subject,
                body_delta: email.body_delta,
                datasets: email.datasets,
                reply_to_noreply: email.reply_to_noreply,
            },
        )
        .await?;

    // Delete the stored email
    let _: common::message::email::DeleteStoredEmailResponse = common::service::request(
        "emailer.stored.delete",
        common::message::email::DeleteStoredEmailRequest { stored_email_id },
    )
    .await?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header(
            "Location",
            format!("/-admin/email/oneoff/{}", create_response.oneoff_email_id),
        )
        .finish())
}

// API endpoint to fetch dataset variables
#[handler]
async fn email_dataset_variables(
    Path(dataset_name): Path<String>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::email::GetDatasetVariablesRequest { dataset_name };

    match common::service::request::<common::message::email::GetDatasetVariablesResponse>(
        "emailer.datasets.variables",
        request,
    )
    .await
    {
        Ok(response) => {
            let json = serde_json::to_string(&response.variables)?;
            Ok(poem::Response::builder()
                .content_type("application/json")
                .body(json))
        }
        Err(e) => Err(anyhow::anyhow!("Failed to fetch dataset variables: {}", e)),
    }
}
