// Address sets and user sets management for admin

use crate::admin::autocomplete;
use crate::component::{AdminLayout, Component};
use crate::form::QsForm;
use crate::middleware::authorization::Authenticated;
use common::timestring::TimeString;
use poem::{
    Endpoint, Route, delete, get, handler, post,
    web::{Form, Path, Query},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

// ============================================================================
// Address Sets Templates
// ============================================================================

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/address_sets.stpl.html")]
pub struct AddressSetsContent {
    pub address_sets: Vec<common::message::user::AddressSetInfo>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/new_address_set.stpl.html")]
pub struct NewAddressSetPage {
    pub error_message: Option<String>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/edit_address_set.stpl.html")]
pub struct EditAddressSetPage {
    pub slug: String,
    pub name: String,
    pub addresses: Vec<String>,
    pub error_message: Option<String>,
}

// ============================================================================
// Address Sets Forms
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CreateAddressSetForm {
    slug: String,
    name: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAddressSetForm {
    name: String,
}

#[derive(Debug, Deserialize)]
pub struct AddMemberForm {
    email_address: String,
}

// ============================================================================
// User Sets Templates
// ============================================================================

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/user_sets.stpl.html")]
pub struct UserSetsContent {
    pub user_sets: Vec<common::message::user::UserSetInfo>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/new_user_set.stpl.html")]
pub struct NewUserSetPage {
    pub error_message: Option<String>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/edit_user_set.stpl.html")]
pub struct EditUserSetPage {
    pub slug: String,
    pub name: String,
    pub criteria: Vec<common::message::user::UserSetCriterionWithId>,
    pub project_names: std::collections::HashMap<Ulid, String>,
    pub user_emails: std::collections::HashMap<Ulid, String>,
    pub error_message: Option<String>,
}

// ============================================================================
// User Sets Forms
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CreateUserSetForm {
    slug: String,
    name: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserSetForm {
    name: String,
}

#[derive(Debug, Deserialize)]
struct CriterionFieldsQuery {
    criterion_type: String,
}

#[derive(Debug, Deserialize)]
pub struct ExplicitListAddForm {
    new_user_id: Option<String>,
    #[serde(default)]
    user_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExplicitListRemoveForm {
    #[serde(default)]
    user_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddCriterionForm {
    criterion_type: String,
    operation: Option<String>,
    tag: Option<String>,
    geojson: Option<String>,
    affiliate_id: Option<String>,
    start: Option<String>,
    end: Option<String>,
    #[serde(default)]
    user_ids: Vec<String>,
}

// ============================================================================
// Criterion Field Templates
// ============================================================================

#[derive(TemplateOnce)]
#[template(path = "admin/partials/criterion_fields/has_tag.stpl.html")]
pub struct HasTagCriterionFields;

#[derive(TemplateOnce)]
#[template(path = "admin/partials/criterion_fields/has_point_in_region.stpl.html")]
pub struct HasPointInRegionCriterionFields;

#[derive(TemplateOnce)]
#[template(path = "admin/partials/criterion_fields/affiliate_participation.stpl.html")]
pub struct AffiliateParticipationCriterionFields {
    widget: autocomplete::EntityAutocomplete,
}

#[derive(TemplateOnce)]
#[template(path = "admin/partials/criterion_fields/is_page_manager.stpl.html")]
pub struct IsPageManagerCriterionFields;

#[derive(TemplateOnce)]
#[template(path = "admin/partials/criterion_fields/is_staff.stpl.html")]
pub struct IsStaffCriterionFields;

#[derive(TemplateOnce)]
#[template(path = "admin/partials/criterion_fields/is_verified.stpl.html")]
pub struct IsVerifiedCriterionFields;

#[derive(TemplateOnce)]
#[template(path = "admin/partials/explicit_list_widget.stpl.html")]
pub struct ExplicitListWidget {
    user_names: Vec<(Ulid, String)>,
    widget: autocomplete::EntityAutocomplete,
}

#[derive(TemplateOnce)]
#[template(path = "admin/partials/criterion_fields/explicit_list.stpl.html")]
pub struct ExplicitListCriterionFields {
    inner: ExplicitListWidget,
}

#[derive(TemplateOnce)]
#[template(path = "admin/partials/user_set_criteria_list.stpl.html")]
pub struct UserSetCriteriaList {
    slug: String,
    criteria: Vec<common::message::user::UserSetCriterionWithId>,
    project_names: std::collections::HashMap<Ulid, String>,
    user_emails: std::collections::HashMap<Ulid, String>,
}

// ============================================================================
// Address Sets Handlers
// ============================================================================

#[handler]
async fn address_sets_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::AddressSetListRequest {};

    let response: common::message::user::AddressSetListResponse =
        common::service::request("user.address_sets.list", request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list address sets: {}", e))?;

    let content = AddressSetsContent {
        address_sets: response.address_sets,
    };

    AdminLayout::new("Address Sets", content)
        .with_breadcrumb("Address Sets", None)
        .with_sidebar_active("email-address-sets")
        .into_response(user_info)
        .await
}

#[handler]
async fn address_sets_new(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = NewAddressSetPage {
        error_message: None,
    };

    AdminLayout::new("New Address Set", content)
        .with_breadcrumb("Address Sets", Some("/-admin/email/address-sets"))
        .with_breadcrumb("New", None)
        .with_sidebar_active("email-address-sets")
        .into_response(user_info)
        .await
}

#[handler]
async fn address_sets_create(
    Form(form): Form<CreateAddressSetForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    let slug = form.slug.clone();
    let request = common::message::user::AddressSetCreateRequest {
        slug: form.slug,
        name: form.name,
    };

    common::service::request::<common::message::user::AddressSetResponse>(
        "user.address_sets.create",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to create address set: {}", e))?;

    common::service::log_activity(
        None,
        "address-set-create",
        user_info.user_id,
        None,
        Some(slug),
    )
    .await?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", "/-admin/email/address-sets")
        .finish())
}

#[handler]
async fn address_sets_edit(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::AddressSetGetRequest {
        slug: slug.clone(),
        page: None,
        limit: None,
    };

    match common::service::request::<common::message::user::AddressSetGetResponse>(
        "user.address_sets.get",
        request,
    )
    .await
    {
        Ok(response) => {
            let content = EditAddressSetPage {
                slug: response.slug,
                name: response.name,
                addresses: response.addresses,
                error_message: None,
            };
            AdminLayout::new("Edit Address Set", content)
                .with_breadcrumb("Address Sets", Some("/-admin/email/address-sets"))
                .with_breadcrumb("Edit", None)
                .with_sidebar_active("email-address-sets")
                .into_response(user_info)
                .await
        }
        Err(e) => Err(anyhow::anyhow!("Failed to load address set: {}", e)),
    }
}

#[handler]
async fn address_sets_update(
    Path(slug): Path<String>,
    Form(form): Form<UpdateAddressSetForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::AddressSetUpdateRequest {
        slug: slug.clone(),
        name: form.name,
    };

    common::service::request::<common::message::user::AddressSetResponse>(
        "user.address_sets.update",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to update address set: {}", e))?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", format!("/-admin/email/address-sets/{}", slug))
        .finish())
}

#[handler]
async fn address_sets_delete(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    let slug_clone = slug.clone();
    let request = common::message::user::AddressSetDeleteRequest { slug };

    common::service::request::<common::message::user::AddressSetResponse>(
        "user.address_sets.delete",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to delete address set: {}", e))?;

    common::service::log_activity(
        None,
        "address-set-delete",
        user_info.user_id,
        None,
        Some(slug_clone),
    )
    .await?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .header("HX-Redirect", "/-admin/email/address-sets")
        .finish())
}

#[handler]
async fn address_sets_add_member(
    Path(slug): Path<String>,
    Form(form): Form<AddMemberForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let email_address = form.email_address.clone();
    let request = common::message::user::AddressSetAddMemberRequest {
        slug: slug.clone(),
        email_address: form.email_address,
    };

    common::service::request::<common::message::user::AddressSetMemberResponse>(
        "user.address_sets.add_member",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to add member: {}", e))?;

    let html = format!(
        r#"<li class="py-3 flex justify-between items-center">
    <span class="text-sm text-gray-900">{}</span>
    <button
        hx-delete="/-admin/email/address-sets/{}/members/{}"
        hx-confirm="Remove {} from this address set?"
        hx-target="closest li"
        hx-swap="outerHTML swap:500ms"
        class="text-sm text-red-600 hover:text-red-900"
    >
        Remove
    </button>
</li>
<div id="empty-state" hx-swap-oob="delete"></div>"#,
        email_address, slug, email_address, email_address
    );

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("text/html")
        .body(html))
}

#[handler]
async fn address_sets_remove_member(
    Path((slug, email)): Path<(String, String)>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::AddressSetRemoveMemberRequest {
        slug: slug.clone(),
        email_address: email,
    };

    common::service::request::<common::message::user::AddressSetMemberResponse>(
        "user.address_sets.remove_member",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to remove member: {}", e))?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .finish())
}

// ============================================================================
// User Sets Handlers
// ============================================================================

#[handler]
async fn user_sets_list(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::UserSetListRequest {};

    let response: common::message::user::UserSetListResponse =
        common::service::request("user.user_sets.list", request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list user sets: {}", e))?;

    let content = UserSetsContent {
        user_sets: response.user_sets,
    };

    AdminLayout::new("User Sets", content)
        .with_breadcrumb("User Sets", None)
        .with_sidebar_active("email-user-sets")
        .into_response(user_info)
        .await
}

#[handler]
async fn user_sets_new(
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let content = NewUserSetPage {
        error_message: None,
    };

    AdminLayout::new("New User Set", content)
        .with_breadcrumb("User Sets", Some("/-admin/email/user-sets"))
        .with_breadcrumb("New", None)
        .with_sidebar_active("email-user-sets")
        .into_response(user_info)
        .await
}

#[handler]
async fn user_sets_create(
    Form(form): Form<CreateUserSetForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    let slug = form.slug.clone();
    let request = common::message::user::UserSetCreateRequest {
        slug: form.slug,
        name: form.name,
    };

    common::service::request::<common::message::user::UserSetResponse>(
        "user.user_sets.create",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to create user set: {}", e))?;

    common::service::log_activity(None, "user-set-create", user_info.user_id, None, Some(slug))
        .await?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", "/-admin/email/user-sets")
        .finish())
}

#[handler]
async fn user_sets_edit(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::UserSetGetRequest {
        slug: slug.clone(),
        page: None,
        limit: None,
        evaluation_id: None,
    };

    match common::service::request::<common::message::user::UserSetDetailResponse>(
        "user.user_sets.get_detail",
        request,
    )
    .await
    {
        Ok(response) => {
            // Extract all project IDs and user IDs from criteria
            let mut project_ids = Vec::new();
            let mut user_ids = Vec::new();

            for criterion in &response.criteria {
                match &criterion.criterion {
                    common::message::user::UserSetCriterion::AffiliateParticipation {
                        affiliate_id,
                        ..
                    } => {
                        project_ids.push(*affiliate_id);
                    }
                    common::message::user::UserSetCriterion::ExplicitList { user_ids: ids } => {
                        user_ids.extend(ids.iter().copied());
                    }
                    _ => {}
                }
            }

            // Look up project names and user emails
            let project_name_list = if !project_ids.is_empty() {
                autocomplete::resolve_project_names(project_ids)
                    .await
                    .unwrap_or_default()
            } else {
                vec![]
            };

            let user_email_list = if !user_ids.is_empty() {
                autocomplete::resolve_user_names(user_ids)
                    .await
                    .unwrap_or_default()
            } else {
                vec![]
            };

            // Convert to HashMaps for easy lookup
            let project_names: std::collections::HashMap<Ulid, String> =
                project_name_list.into_iter().collect();
            let user_emails: std::collections::HashMap<Ulid, String> =
                user_email_list.into_iter().collect();

            let content = EditUserSetPage {
                slug: response.slug,
                name: response.name,
                criteria: response.criteria,
                project_names,
                user_emails,
                error_message: None,
            };
            AdminLayout::new("Edit User Set", content)
                .with_breadcrumb("User Sets", Some("/-admin/email/user-sets"))
                .with_breadcrumb("Edit", None)
                .with_sidebar_active("email-user-sets")
                .into_response(user_info)
                .await
        }
        Err(e) => Err(anyhow::anyhow!("Failed to load user set: {}", e)),
    }
}

#[handler]
async fn user_sets_update(
    Path(slug): Path<String>,
    Form(form): Form<UpdateUserSetForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let request = common::message::user::UserSetUpdateRequest {
        slug: slug.clone(),
        name: form.name,
    };

    common::service::request::<common::message::user::UserSetResponse>(
        "user.user_sets.update",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to update user set: {}", e))?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::SEE_OTHER)
        .header("Location", format!("/-admin/email/user-sets/{}", slug))
        .finish())
}

#[handler]
async fn user_sets_delete(
    Path(slug): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let user_info = user_info.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

    let slug_clone = slug.clone();
    let request = common::message::user::UserSetDeleteRequest { slug };

    common::service::request::<common::message::user::UserSetResponse>(
        "user.user_sets.delete",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to delete user set: {}", e))?;

    common::service::log_activity(
        None,
        "user-set-delete",
        user_info.user_id,
        None,
        Some(slug_clone),
    )
    .await?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .header("HX-Redirect", "/-admin/email/user-sets")
        .finish())
}

#[handler]
async fn user_sets_criterion_fields(
    Query(query): Query<CriterionFieldsQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use common::message::user::UserSetCriterionType;
    use std::str::FromStr;

    let criterion_type = UserSetCriterionType::from_str(&query.criterion_type)
        .map_err(|_| anyhow::anyhow!("Invalid criterion type: {}", query.criterion_type))?;

    let html = match criterion_type {
        UserSetCriterionType::HasTag => HasTagCriterionFields.render_once()?,
        UserSetCriterionType::HasPointInRegion => HasPointInRegionCriterionFields.render_once()?,
        UserSetCriterionType::AffiliateParticipation => {
            let widget = autocomplete::EntityAutocomplete::new_project_selector(
                "affiliate_id",
                "Affiliate Project",
                "Search for project...",
            );
            AffiliateParticipationCriterionFields { widget }.render_once()?
        }
        UserSetCriterionType::IsPageManager => IsPageManagerCriterionFields.render_once()?,
        UserSetCriterionType::IsStaff => IsStaffCriterionFields.render_once()?,
        UserSetCriterionType::IsVerified => IsVerifiedCriterionFields.render_once()?,
        UserSetCriterionType::ExplicitList => {
            let widget = autocomplete::EntityAutocomplete::new_user_selector(
                "new_user_id",
                "",
                "Search for user to add...",
            );
            let inner = ExplicitListWidget {
                user_names: vec![],
                widget,
            };
            ExplicitListCriterionFields { inner }.render_once()?
        }
    };

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("text/html")
        .body(html))
}

async fn render_explicit_list_widget(user_ids: Vec<Ulid>) -> Result<String, anyhow::Error> {
    // Resolve user names
    let user_names = if !user_ids.is_empty() {
        autocomplete::resolve_user_names(user_ids.clone()).await?
    } else {
        vec![]
    };

    let widget = autocomplete::EntityAutocomplete::new_user_selector(
        "new_user_id",
        "",
        "Search for user to add...",
    );

    let template = ExplicitListWidget { user_names, widget };

    Ok(template.render_once()?)
}

#[handler]
async fn user_sets_explicit_list_add_user(
    QsForm(form): QsForm<ExplicitListAddForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let mut user_ids: Vec<Ulid> = form
        .user_ids
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect();

    if let Some(new_id_str) = form.new_user_id {
        if let Ok(new_id) = new_id_str.parse::<Ulid>() {
            if !user_ids.contains(&new_id) {
                user_ids.push(new_id);
            }
        }
    }

    let html = render_explicit_list_widget(user_ids).await?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("text/html")
        .body(html))
}

#[handler]
async fn user_sets_explicit_list_remove_user(
    Path(user_id_to_remove): Path<String>,
    QsForm(form): QsForm<ExplicitListRemoveForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let remove_id = user_id_to_remove
        .parse::<Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID"))?;

    let user_ids: Vec<Ulid> = form
        .user_ids
        .iter()
        .filter_map(|s| s.parse().ok())
        .filter(|id| *id != remove_id)
        .collect();

    let html = render_explicit_list_widget(user_ids).await?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("text/html")
        .body(html))
}

#[handler]
async fn user_sets_add_criterion(
    Path(slug): Path<String>,
    QsForm(form): QsForm<AddCriterionForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    use common::message::user::UserSetCriterionType;
    use std::str::FromStr;

    // Parse the criterion type from string
    let criterion_type = UserSetCriterionType::from_str(&form.criterion_type)
        .map_err(|_| anyhow::anyhow!("Invalid criterion type: {}", form.criterion_type))?;

    // Parse criterion data based on type
    let criterion = match criterion_type {
        UserSetCriterionType::HasTag => {
            let tag = form.tag.ok_or_else(|| anyhow::anyhow!("Missing tag"))?;
            common::message::user::UserSetCriterion::HasTag { tag }
        }
        UserSetCriterionType::HasPointInRegion => {
            let geojson = form
                .geojson
                .ok_or_else(|| anyhow::anyhow!("Missing geojson"))?;
            common::message::user::UserSetCriterion::HasPointInRegion { geojson }
        }
        UserSetCriterionType::AffiliateParticipation => {
            let affiliate_id_str = form
                .affiliate_id
                .ok_or_else(|| anyhow::anyhow!("Missing affiliate_id"))?;
            let affiliate_id = affiliate_id_str
                .parse::<Ulid>()
                .map_err(|e| anyhow::anyhow!("Invalid affiliate_id: {}", e))?;

            let start_str = form
                .start
                .ok_or_else(|| anyhow::anyhow!("Missing start date"))?;
            let start = TimeString::try_from(start_str)
                .map_err(|e| anyhow::anyhow!("Invalid start time: {}", e))?;

            let end_str = form
                .end
                .ok_or_else(|| anyhow::anyhow!("Missing end date"))?;
            let end = TimeString::try_from(end_str)
                .map_err(|e| anyhow::anyhow!("Invalid end time: {}", e))?;

            common::message::user::UserSetCriterion::AffiliateParticipation {
                affiliate_id,
                start,
                end,
            }
        }
        UserSetCriterionType::IsPageManager => {
            common::message::user::UserSetCriterion::IsPageManager
        }
        UserSetCriterionType::IsStaff => common::message::user::UserSetCriterion::IsStaff,
        UserSetCriterionType::IsVerified => common::message::user::UserSetCriterion::IsVerified,
        UserSetCriterionType::ExplicitList => {
            if form.user_ids.is_empty() {
                return Err(anyhow::anyhow!("Missing user IDs"));
            }
            let parsed_ids: Result<Vec<Ulid>, _> =
                form.user_ids.iter().map(|s| s.parse::<Ulid>()).collect();
            let user_ids = parsed_ids.map_err(|e| anyhow::anyhow!("Invalid ULID: {}", e))?;

            common::message::user::UserSetCriterion::ExplicitList { user_ids }
        }
    };

    // Parse the operation type
    let operation = form.operation.as_deref().unwrap_or("union");
    let operation =
        common::message::user::CriterionOperation::from_str(operation).map_err(|_| {
            anyhow::anyhow!(
                "Invalid operation: {}",
                form.operation.as_deref().unwrap_or("unknown")
            )
        })?;

    let request = common::message::user::UserSetAddCriterionRequest {
        slug: slug.clone(),
        criterion,
        operation,
    };

    common::service::request::<common::message::user::UserSetCriterionResponse>(
        "user.user_sets.add_criterion",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to add criterion: {}", e))?;

    // Fetch updated criteria list
    let get_request = common::message::user::UserSetGetRequest {
        slug: slug.clone(),
        page: None,
        limit: None,
        evaluation_id: None,
    };
    let response = common::service::request::<common::message::user::UserSetDetailResponse>(
        "user.user_sets.get_detail",
        get_request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to fetch updated user set: {}", e))?;

    // Extract all project IDs and user IDs from criteria
    let mut project_ids = Vec::new();
    let mut user_ids = Vec::new();

    for criterion in &response.criteria {
        match &criterion.criterion {
            common::message::user::UserSetCriterion::AffiliateParticipation {
                affiliate_id,
                ..
            } => {
                project_ids.push(*affiliate_id);
            }
            common::message::user::UserSetCriterion::ExplicitList { user_ids: ids } => {
                user_ids.extend(ids.iter().copied());
            }
            _ => {}
        }
    }

    // Look up project names and user emails
    let project_name_list = if !project_ids.is_empty() {
        autocomplete::resolve_project_names(project_ids)
            .await
            .unwrap_or_default()
    } else {
        vec![]
    };

    let user_email_list = if !user_ids.is_empty() {
        autocomplete::resolve_user_names(user_ids)
            .await
            .unwrap_or_default()
    } else {
        vec![]
    };

    // Convert to HashMaps for easy lookup
    let project_names: std::collections::HashMap<Ulid, String> =
        project_name_list.into_iter().collect();
    let user_emails: std::collections::HashMap<Ulid, String> =
        user_email_list.into_iter().collect();

    // Render criteria list
    let criteria_list = UserSetCriteriaList {
        slug: slug.clone(),
        criteria: response.criteria,
        project_names,
        user_emails,
    };
    let list_html = criteria_list.render_once()?;

    // Render fresh criterion fields for the same type (OOB swap)
    let fields_html = match criterion_type {
        UserSetCriterionType::HasTag => HasTagCriterionFields.render_once()?,
        UserSetCriterionType::HasPointInRegion => HasPointInRegionCriterionFields.render_once()?,
        UserSetCriterionType::AffiliateParticipation => {
            let widget = autocomplete::EntityAutocomplete::new_project_selector(
                "affiliate_id",
                "Affiliate Project",
                "Search for project...",
            );
            AffiliateParticipationCriterionFields { widget }.render_once()?
        }
        UserSetCriterionType::IsPageManager => IsPageManagerCriterionFields.render_once()?,
        UserSetCriterionType::IsStaff => IsStaffCriterionFields.render_once()?,
        UserSetCriterionType::IsVerified => IsVerifiedCriterionFields.render_once()?,
        UserSetCriterionType::ExplicitList => {
            let widget = autocomplete::EntityAutocomplete::new_user_selector(
                "new_user_id",
                "",
                "Search for user to add...",
            );
            let inner = ExplicitListWidget {
                user_names: vec![],
                widget,
            };
            ExplicitListCriterionFields { inner }.render_once()?
        }
    };

    // Combine with OOB swap
    let response_html = format!(
        "{}\n<div id=\"criterion-fields-container\" hx-swap-oob=\"true\">{}</div>",
        list_html, fields_html
    );

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("text/html")
        .body(response_html))
}

#[handler]
async fn user_sets_remove_criterion(
    Path((slug, id)): Path<(String, String)>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let criterion_id = id
        .parse::<i64>()
        .map_err(|_| anyhow::anyhow!("Invalid criterion ID"))?;

    let request = common::message::user::UserSetRemoveCriterionRequest {
        slug: slug.clone(),
        criterion_id,
    };

    common::service::request::<common::message::user::UserSetCriterionResponse>(
        "user.user_sets.remove_criterion",
        request,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to remove criterion: {}", e))?;

    Ok(poem::Response::builder()
        .status(poem::http::StatusCode::OK)
        .finish())
}

// ============================================================================
// Routes
// ============================================================================

pub fn address_sets_routes() -> impl Endpoint {
    Route::new()
        .at("/", get(address_sets_list))
        .at("/new", get(address_sets_new).post(address_sets_create))
        .at(
            "/:slug",
            get(address_sets_edit)
                .post(address_sets_update)
                .delete(address_sets_delete),
        )
        .at("/:slug/members", post(address_sets_add_member))
        .at("/:slug/members/:email", delete(address_sets_remove_member))
}

pub fn user_sets_routes() -> impl Endpoint {
    Route::new()
        .at("/", get(user_sets_list))
        .at("/new", get(user_sets_new).post(user_sets_create))
        .at("/criterion-fields", get(user_sets_criterion_fields))
        .at(
            "/explicit-list-widget",
            post(user_sets_explicit_list_add_user),
        )
        .at(
            "/explicit-list-widget/:user_id",
            delete(user_sets_explicit_list_remove_user),
        )
        .at(
            "/:slug",
            get(user_sets_edit)
                .post(user_sets_update)
                .delete(user_sets_delete),
        )
        .at("/:slug/criteria", post(user_sets_add_criterion))
        .at("/:slug/criteria/:id", delete(user_sets_remove_criterion))
}
