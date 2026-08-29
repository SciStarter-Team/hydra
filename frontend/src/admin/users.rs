//! User management handlers for the admin interface.
//!
//! This module handles user CRUD operations, user search/listing,
//! password reset, and user email management.

use crate::component::{AdminLayout, Component, admin_layout::AdminLayoutPrepared};
use crate::middleware::authorization::Authenticated;
use crate::middleware::error_pages::error_response;
use common::message::user::{
    AdminPasswordResetRequest, AdminPasswordResetResponse, UserDetails, UserDetailsCreateRequest,
    UserDetailsDeleteRequest, UserDetailsDeleteResponse, UserDetailsGetRequest,
    UserDetailsListRequest, UserDetailsListResponse, UserDetailsResponse, UserDetailsSearchRequest,
    UserDetailsUpdateRequest, UserEmail, UserEmailAddRequest, UserEmailDeleteRequest,
    UserEmailResponse, UserEmailToggleVerifiedRequest, UserEmailUpdateRequest,
};
use common::service::{request, request_with_retry};
use poem::handler;
use poem::web::{Form, Path, Query};
use poem::{Route, get, post, put};
use sailfish::TemplateOnce;
use serde::Deserialize;
use std::sync::Arc;

pub fn routes() -> Route {
    Route::new()
        .at("/", get(users))
        .at("/search", get(users_search))
        .at("/new", get(users_new).post(users_create))
        .at(
            "/:id",
            // Updating a user is a PUT. POST is also accepted because the edit
            // page submits as a plain <form>, and HTML forms can only issue GET
            // or POST; the modal, which goes through htmx, uses PUT.
            get(users_edit)
                .put(users_update)
                .post(users_update)
                .delete(users_delete),
        )
        .at("/:id/send-password-reset", post(users_password_reset))
        .at("/:id/emails", post(users_emails_add))
        // Addressed by the row's own id, not by the address. An address is a
        // natural key rather than an identity: it changes when edited, and it
        // is not reliably a URL path segment -- an empty one is no segment at
        // all, which once left a row impossible to name and so to delete.
        .at(
            "/:id/emails/:email_id",
            put(users_emails_update).delete(users_emails_delete),
        )
        .at(
            "/:id/emails/:email_id/toggle-verified",
            post(users_emails_toggle_verified),
        )
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/users.stpl.html")]
pub struct UsersPagePrepared {
    pub users: Vec<UserDetails>,
    pub current_page: u32,
    pub total_users: u64,
    pub has_previous: bool,
    pub has_next: bool,
    pub per_page: u32,
    pub start_item: u32,
    pub end_item: u32,
    pub pagination_url: String,
    pub pagination_include: String,
    pub search_term: String,
    pub selected_role: String,
    pub selected_email_status: String,
}

#[derive(Debug)]
pub struct UsersPage {
    page: Option<u32>,
    search: Option<String>,
    role: Option<String>,
    email_status: Option<String>,
}

impl Component for UsersPage {
    type Param = Option<Arc<common::message::user::BasicInfo>>;
    type Prepared = AdminLayoutPrepared<UsersPagePrepared>;

    async fn prepare(self, param: Self::Param) -> Result<Self::Prepared, anyhow::Error> {
        // Fetch real user data from user service
        let page = self.page.unwrap_or(1);
        let user_request = UserDetailsListRequest {
            page: Some(page),
            limit: Some(25),
        };

        let response: UserDetailsListResponse = request("user.admin.user_list", user_request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch users: {}", e))?;

        let users_list = response.users;

        let users_content = UsersContent {
            users: users_list,
            current_page: response.page,
            total_users: response.total_count,
            has_previous: response.page > 1,
            has_next: (response.page * response.limit) < response.total_count as u32,
            per_page: response.limit,
            start_item: ((response.page - 1) * response.limit) + 1,
            end_item: std::cmp::min(response.page * response.limit, response.total_count as u32),
            pagination_url: "/-admin/users".to_string(),
            pagination_include: "#user-search-form".to_string(),
            search_term: "".to_string(),
            selected_role: "".to_string(),
            selected_email_status: "".to_string(),
        };

        let users_page = UsersPagePrepared {
            users: users_content.users,
            current_page: users_content.current_page,
            total_users: users_content.total_users,
            has_previous: users_content.has_previous,
            has_next: users_content.has_next,
            per_page: users_content.per_page,
            start_item: users_content.start_item,
            end_item: users_content.end_item,
            pagination_url: users_content.pagination_url,
            pagination_include: users_content.pagination_include,
            search_term: self.search.unwrap_or_default(),
            selected_role: self.role.unwrap_or_default(),
            selected_email_status: self.email_status.unwrap_or_default(),
        };

        AdminLayout::new("Users", users_page)
            .with_breadcrumb("Users", None)
            .with_sidebar_active("users")
            .prepare(param)
            .await
    }
}

#[derive(Debug, Deserialize)]
struct UserPageQuery {
    page: Option<u32>,
    search: Option<String>,
    role: Option<String>,
    email_status: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct UserSearchQuery {
    search: Option<String>,
    role: Option<String>,
    email_status: Option<String>,
    page: Option<u32>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/users_table_body.stpl.html")]
pub struct UsersTableBody {
    users: Vec<UserDetails>,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/users_content.stpl.html")]
pub struct UsersContent {
    pub users: Vec<UserDetails>,
    pub current_page: u32,
    pub total_users: u64,
    pub has_previous: bool,
    pub has_next: bool,
    pub per_page: u32,
    pub start_item: u32,
    pub end_item: u32,
    pub pagination_url: String,
    pub pagination_include: String,
    pub search_term: String,
    pub selected_role: String,
    pub selected_email_status: String,
}

#[handler]
async fn users(
    Query(query): Query<UserPageQuery>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // If any search parameters are provided, perform a search instead of the default list
    if query.search.is_some() || query.role.is_some() || query.email_status.is_some() {
        let search_query = UserSearchQuery {
            search: query.search,
            role: query.role,
            email_status: query.email_status,
            page: query.page,
        };
        let users_page = users_search_internal(search_query).await?;

        return AdminLayout::new("Users", users_page)
            .with_breadcrumb("Users", None)
            .with_sidebar_active("users")
            .into_response(user_info)
            .await;
    }

    UsersPage {
        page: query.page,
        search: query.search,
        role: query.role,
        email_status: query.email_status,
    }
    .into_response(user_info)
    .await
}

async fn users_search_internal(query: UserSearchQuery) -> Result<UsersPagePrepared, anyhow::Error> {
    let page = query.page.unwrap_or(1);
    let search_request = UserDetailsSearchRequest {
        search: query.search.clone(),
        role: query.role.clone(),
        email_status: query.email_status.clone(),
        page: Some(page),
        limit: Some(25),
    };

    let response: UserDetailsListResponse = request("user.admin.user_search", search_request)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to search users: {}", e))?;

    let users_list = response.users;

    // Calculate pagination info
    let total_pages = (response.total_count + 24) / 25; // Round up
    let has_previous = page > 1;
    let has_next = page < total_pages as u32;
    let start_item = if response.total_count == 0 {
        0
    } else {
        ((page - 1) * 25) + 1
    };
    let end_item = std::cmp::min(page * 25, response.total_count as u32);

    Ok(UsersPagePrepared {
        users: users_list,
        current_page: page,
        total_users: response.total_count,
        has_previous,
        has_next,
        per_page: response.limit,
        start_item,
        end_item,
        pagination_url: "/-admin/users".to_string(),
        pagination_include: "[name='search'], [name='role'], [name='email_status']".to_string(),
        search_term: query.search.unwrap_or_default(),
        selected_role: query.role.unwrap_or_default(),
        selected_email_status: query.email_status.unwrap_or_default(),
    })
}

#[handler]
async fn users_search(
    Query(query): Query<UserSearchQuery>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let users_page = users_search_internal(query.clone()).await?;

    // For HTMX requests, render just the users content and set URL push header
    let users_content = UsersContent {
        users: users_page.users,
        current_page: users_page.current_page,
        total_users: users_page.total_users,
        has_previous: users_page.has_previous,
        has_next: users_page.has_next,
        per_page: users_page.per_page,
        start_item: users_page.start_item,
        end_item: users_page.end_item,
        pagination_url: users_page.pagination_url,
        pagination_include: users_page.pagination_include,
        search_term: query.search.as_deref().unwrap_or("").to_string(),
        selected_role: query.role.as_deref().unwrap_or("").to_string(),
        selected_email_status: query.email_status.as_deref().unwrap_or("").to_string(),
    };

    let html = users_content.render_once()?;

    // Build URL parameters for HX-Push-Url
    let mut url_params = vec![];
    if let Some(search) = &query.search {
        if !search.trim().is_empty() {
            url_params.push(format!("search={}", urlencoding::encode(search)));
        }
    }
    if let Some(role) = &query.role {
        if !role.trim().is_empty() {
            url_params.push(format!("role={}", urlencoding::encode(role)));
        }
    }
    if let Some(email_status) = &query.email_status {
        if !email_status.trim().is_empty() {
            url_params.push(format!(
                "email_status={}",
                urlencoding::encode(email_status)
            ));
        }
    }
    let page = query.page.unwrap_or(1);
    if page > 1 {
        url_params.push(format!("page={}", page));
    }

    let url = if url_params.is_empty() {
        "/-admin/users".to_string()
    } else {
        format!("/-admin/users?{}", url_params.join("&"))
    };

    Ok(poem::Response::builder()
        .content_type("text/html")
        .header("HX-Push-Url", url)
        .body(html))
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/modals/new_user.stpl.html")]
struct NewUserForm {
    title: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/pages/edit_user.stpl.html")]
struct EditUserPage {
    user_id: String,
    first_name: String,
    last_name: String,
    email: String,
    handle: String,
    role: String,
    active: bool,
    emails: Vec<UserEmail>,
    error_message: Option<String>,
    orcid: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateUserForm {
    first_name: String,
    last_name: String,
    email: String,
    handle: String,
    password: String,
    role: String,
    #[serde(default)]
    verified: bool,
}

#[derive(Debug, Deserialize)]
struct UpdateUserForm {
    first_name: String,
    last_name: String,
    email: String,
    handle: String,
    new_password: Option<String>,
    role: String,
    #[serde(default)]
    active: bool,
    orcid: Option<String>,
}

#[handler]
async fn users_new(
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let new_user_form = NewUserForm {
        title: "Add New User".to_string(),
    };

    let html = new_user_form.render_once()?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(html))
}

#[handler]
async fn users_edit(
    Path(user_id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Parse user ID
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    // Fetch user data from user service
    let get_request = UserDetailsGetRequest {
        user_id: parsed_user_id,
    };

    let content = match request::<UserDetailsResponse>("user.admin.user_get", get_request).await {
        Ok(response) => {
            let admin_user = response.user;
            EditUserPage {
                user_id,
                first_name: admin_user.first_name,
                last_name: admin_user.last_name,
                email: admin_user.primary_email.clone(),
                handle: admin_user.handle,
                role: if admin_user.is_superuser {
                    "staff".to_string()
                } else if admin_user.is_page_manager {
                    "page_manager".to_string()
                } else {
                    "user".to_string()
                },
                active: admin_user.is_active,
                emails: admin_user.emails,
                error_message: None,
                orcid: admin_user.orcid,
            }
        }
        Err(_e) => {
            // If user not found, return empty form with error state
            EditUserPage {
                user_id,
                first_name: "".to_string(),
                last_name: "".to_string(),
                email: "".to_string(),
                handle: "".to_string(),
                role: "user".to_string(),
                active: true,
                emails: vec![],
                error_message: Some("User not found".to_string()),
                orcid: None,
            }
        }
    };

    let prepared = crate::component::AdminLayout::new("Edit User", content)
        .with_sidebar_active("users")
        .with_breadcrumb("Users", Some("/-admin/users"))
        .with_breadcrumb("Edit", None)
        .prepare(user_info)
        .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[handler]
async fn users_delete(
    Path(user_id): Path<String>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Parse user ID
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let delete_request = UserDetailsDeleteRequest {
        user_id: parsed_user_id,
    };

    match request_with_retry::<UserDetailsDeleteResponse>("user.admin.user_delete", delete_request)
        .await
    {
        Ok(response) => {
            if response.success {
                // Log the activity
                if let Some(user_info) = &user_info {
                    common::service::log_activity(
                        None,
                        "user-delete",
                        user_info.user_id,
                        parsed_user_id,
                        None,
                    )
                    .await?;
                }

                // Success - return empty response to remove the table row
                Ok(poem::Response::builder().content_type("text/html").body(""))
            } else {
                // Return error message
                Ok(error_response(
                    poem::http::StatusCode::BAD_REQUEST,
                    "Failed to delete user",
                ))
            }
        }
        Err(_) => {
            // This should never happen since request_with_retry retries indefinitely
            // But we include it for completeness
            Ok(error_response(
                poem::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Service error",
            ))
        }
    }
}

#[derive(Debug, Deserialize)]
struct PasswordResetForm {
    reset_email: String,
}

#[derive(Debug, Deserialize)]
struct AddEmailForm {
    new_email: String,
}

#[derive(Debug, Deserialize)]
struct UpdateEmailForm {
    new_email: String,
}

#[handler]
async fn users_password_reset(
    Path(user_id): Path<String>,
    Form(form): Form<PasswordResetForm>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Parse user ID
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let reset_request = AdminPasswordResetRequest {
        user_id: parsed_user_id,
        email: form.reset_email,
    };

    match request_with_retry::<AdminPasswordResetResponse>(
        "user.admin.password_reset",
        reset_request,
    )
    .await
    {
        Ok(response) => {
            if response.success {
                // Success - show success message via out-of-band update
                let success_message = format!(
                    r#"<div class="mb-4 p-3 text-sm text-green-700 bg-green-100 rounded-lg border border-green-200" hx-swap-oob="innerHTML:#modal-message-container">
                        <strong>Success:</strong> Password reset email sent successfully: {}
                    </div>"#,
                    response.message
                );
                Ok(poem::Response::builder()
                    .content_type("text/html")
                    .body(success_message))
            } else {
                // Return error message
                Ok(error_response(
                    poem::http::StatusCode::BAD_REQUEST,
                    &format!("Failed to send password reset email: {}", response.message),
                ))
            }
        }
        Err(e) => {
            // Return error response
            Ok(error_response(
                poem::http::StatusCode::INTERNAL_SERVER_ERROR,
                &e.to_string(),
            ))
        }
    }
}

#[handler]
async fn users_create(
    Form(form): Form<CreateUserForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Map role string to boolean flags
    let (_is_staff, _is_page_manager) = match form.role.as_str() {
        "staff" => (true, false),
        "page_manager" => (false, true),
        "user" | _ => (false, false),
    };

    let create_request = UserDetailsCreateRequest {
        handle: form.handle.clone(),
        first_name: form.first_name,
        last_name: form.last_name,
        primary_email: form.email,
        password: form.password,
        role: form.role.clone(),
        verified: form.verified,
    };

    match request_with_retry::<UserDetailsResponse>("user.admin.user_create", create_request).await
    {
        Ok(response) => {
            // Log the activity
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "user-create",
                    user_info.user_id,
                    response.user.user_id,
                    format!("handle={} role={}", form.handle, form.role),
                )
                .await?;
            }
            // Success - close modal and refresh users table with updated data
            match request::<UserDetailsListResponse>(
                "user.admin.user_list",
                UserDetailsListRequest {
                    page: Some(1),
                    limit: Some(25),
                },
            )
            .await
            {
                Ok(users_response) => {
                    let start_item = if users_response.total_count == 0 {
                        0
                    } else {
                        (users_response.page - 1) * users_response.limit + 1
                    };
                    let end_item = std::cmp::min(
                        users_response.page * users_response.limit,
                        users_response.total_count as u32,
                    );

                    let users_content = UsersContent {
                        users: users_response.users,
                        current_page: users_response.page,
                        total_users: users_response.total_count,
                        has_previous: users_response.page > 1,
                        has_next: u64::from(users_response.page * users_response.limit)
                            < users_response.total_count,
                        per_page: users_response.limit,
                        start_item,
                        end_item,
                        pagination_url: "/-admin/users".to_string(),
                        pagination_include: "#user-search-form".to_string(),
                        search_term: "".to_string(),
                        selected_role: "".to_string(),
                        selected_email_status: "".to_string(),
                    };

                    let users_html = users_content.render_once()?;
                    let response_html = format!(
                        r#"
                        <div hx-swap-oob="delete:dialog"></div>
                        <div hx-swap-oob="innerHTML:#users-content">{}</div>
                        "#,
                        users_html
                    );

                    Ok(poem::Response::builder()
                        .content_type("text/html")
                        .body(response_html))
                }
                Err(_) => {
                    // Fallback - just close modal if we can't refresh the list
                    Ok(poem::Response::builder()
                        .content_type("text/html")
                        .body(r#"<div hx-swap-oob="delete:dialog"></div>"#))
                }
            }
        }
        Err(e) => {
            // The service's own words -- "Email address is already in use" and
            // the like -- are what the reader needs, so they are passed through
            // rather than replaced with a generic failure.
            Ok(error_response(
                poem::http::StatusCode::BAD_REQUEST,
                &e.to_string(),
            ))
        }
    }
}

#[handler]
async fn users_update(
    Path(user_id): Path<String>,
    Form(form): Form<UpdateUserForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    // Parse user ID
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    // Validate
    if form.email.trim().is_empty() {
        let content = EditUserPage {
            user_id: user_id.clone(),
            first_name: form.first_name,
            last_name: form.last_name,
            email: form.email,
            handle: form.handle,
            role: form.role,
            active: form.active,
            emails: vec![],
            error_message: Some("Email is required".to_string()),
            orcid: form.orcid,
        };

        let prepared = crate::component::AdminLayout::new("Edit User", content)
            .with_sidebar_active("users")
            .with_breadcrumb("Users", Some("/-admin/users"))
            .with_breadcrumb("Edit", None)
            .prepare(user_info)
            .await?;

        return Ok(poem::Response::builder()
            .content_type("text/html")
            .body(prepared.render_once()?));
    }

    // Normalize empty ORCID to None
    let orcid = form.orcid.clone().filter(|o| !o.trim().is_empty());

    // Update
    let update_request = UserDetailsUpdateRequest {
        user_id: parsed_user_id,
        handle: form.handle.clone(),
        first_name: form.first_name.clone(),
        last_name: form.last_name.clone(),
        primary_email: form.email.clone(),
        new_password: form.new_password.clone().filter(|p| !p.trim().is_empty()),
        role: form.role.clone(),
        verified: false, // Will be auto-calculated by backend based on email verification
        is_active: form.active,
        orcid,
    };

    match request_with_retry::<UserDetailsResponse>("user.admin.user_update", update_request).await
    {
        Ok(_response) => {
            // Log the activity
            if let Some(user_info) = &user_info {
                common::service::log_activity(
                    None,
                    "user-update",
                    user_info.user_id,
                    parsed_user_id,
                    format!("handle={} role={}", form.handle, form.role),
                )
                .await?;
            }

            // Redirect back to list page
            Ok(poem::Response::builder()
                .status(poem::http::StatusCode::SEE_OTHER)
                .header("Location", "/-admin/users")
                .finish())
        }
        Err(e) => {
            // Show error on page
            let content = EditUserPage {
                user_id: user_id.clone(),
                first_name: form.first_name,
                last_name: form.last_name,
                email: form.email,
                handle: form.handle,
                role: form.role,
                active: form.active,
                emails: vec![],
                error_message: Some(format!("Failed to update user: {}", e)),
                orcid: form.orcid,
            };

            let prepared = crate::component::AdminLayout::new("Edit User", content)
                .with_sidebar_active("users")
                .with_breadcrumb("Users", Some("/-admin/users"))
                .with_breadcrumb("Edit", None)
                .prepare(user_info)
                .await?;

            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(prepared.render_once()?))
        }
    }
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/email_row.stpl.html")]
struct EmailRow {
    user_id: String,
    email: UserEmail,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/user_row.stpl.html")]
pub struct UserRow {
    pub user: UserDetails,
    pub oob: bool,
    pub oob_swap: String,
}

#[derive(Debug, TemplateOnce)]
#[template(path = "admin/partials/modal_error_message.stpl.html")]
struct ModalErrorMessage {
    message: String,
}

#[handler]
async fn users_emails_add(
    Path(user_id): Path<String>,
    Form(form): Form<AddEmailForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let email_request = UserEmailAddRequest {
        user_id: parsed_user_id,
        email: form.new_email.clone(),
    };

    match request_with_retry::<UserEmailResponse>("user.admin.email_add", email_request).await {
        Ok(response) => {
            if response.success {
                // Log the activity
                if let Some(user_info) = &user_info {
                    common::service::log_activity(
                        None,
                        "user-email-add",
                        user_info.user_id,
                        parsed_user_id,
                        format!("email={}", form.new_email),
                    )
                    .await?;
                }

                // Find the newly added email by matching the address
                if let Some(new_email) = response.emails.iter().find(|e| e.email == form.new_email)
                {
                    let new_email_html = EmailRow {
                        user_id: user_id.clone(),
                        email: new_email.clone(),
                    }
                    .render_once()
                    .unwrap_or_default();

                    // Include OOB swap to clear any previous error message
                    let response_html = format!(
                        r#"{}<div id="modal-message-container" hx-swap-oob="innerHTML"></div>"#,
                        new_email_html
                    );

                    Ok(poem::Response::builder()
                        .content_type("text/html")
                        .body(response_html))
                } else {
                    let error_html = ModalErrorMessage {
                        message: "No email returned".to_string(),
                    }
                    .render_once()?;
                    Ok(poem::Response::builder()
                        .content_type("text/html")
                        .body(error_html))
                }
            } else {
                // Return 200 OK with OOB swap to show error in modal-message-container
                let error_html = ModalErrorMessage {
                    message: response.message,
                }
                .render_once()?;
                Ok(poem::Response::builder()
                    .content_type("text/html")
                    .body(error_html))
            }
        }
        Err(e) => {
            // Return 200 OK with OOB swap to show error in modal-message-container
            let error_html = ModalErrorMessage {
                message: e.to_string(),
            }
            .render_once()?;
            Ok(poem::Response::builder()
                .content_type("text/html")
                .body(error_html))
        }
    }
}

#[handler]
async fn users_emails_update(
    Path((user_id, email_id)): Path<(String, i64)>,
    Form(form): Form<UpdateEmailForm>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let new_email_clone = form.new_email.clone();
    let email_request = UserEmailUpdateRequest {
        user_id: parsed_user_id,
        email_id,
        new_email: form.new_email,
    };

    match request_with_retry::<UserEmailResponse>("user.admin.email_update", email_request).await {
        Ok(response) => {
            if response.success {
                // Log the activity
                if let Some(user_info) = &user_info {
                    common::service::log_activity(
                        None,
                        "user-email-update",
                        user_info.user_id,
                        parsed_user_id,
                        format!("email_id={email_id} new={new_email_clone}"),
                    )
                    .await?;
                }
                // Find the updated email and render just that row
                if let Some(updated_email) = response
                    .emails
                    .into_iter()
                    .find(|email| email.email == new_email_clone)
                {
                    let row_html = EmailRow {
                        user_id: user_id.clone(),
                        email: updated_email,
                    }
                    .render_once()
                    .unwrap_or_default();

                    Ok(poem::Response::builder()
                        .content_type("text/html")
                        .body(row_html))
                } else {
                    Ok(error_response(
                        poem::http::StatusCode::INTERNAL_SERVER_ERROR,
                        "",
                    ))
                }
            } else {
                Ok(error_response(
                    poem::http::StatusCode::BAD_REQUEST,
                    &response.message,
                ))
            }
        }
        Err(e) => Ok(error_response(
            poem::http::StatusCode::INTERNAL_SERVER_ERROR,
            &e.to_string(),
        )),
    }
}

#[handler]
async fn users_emails_delete(
    Path((user_id, email_id)): Path<(String, i64)>,
    Authenticated(user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let email_request = UserEmailDeleteRequest {
        user_id: parsed_user_id,
        email_id,
    };

    match request_with_retry::<UserEmailResponse>("user.admin.email_delete", email_request).await {
        Ok(response) => {
            if response.success {
                // Log the activity
                if let Some(user_info) = &user_info {
                    common::service::log_activity(
                        None,
                        "user-email-delete",
                        user_info.user_id,
                        parsed_user_id,
                        format!("email_id={email_id}"),
                    )
                    .await?;
                }
                // Return empty content to remove the row
                Ok(poem::Response::builder()
                    .content_type("text/html")
                    .body("".to_string()))
            } else {
                Ok(error_response(
                    poem::http::StatusCode::BAD_REQUEST,
                    &response.message,
                ))
            }
        }
        Err(e) => Ok(error_response(
            poem::http::StatusCode::INTERNAL_SERVER_ERROR,
            &e.to_string(),
        )),
    }
}

#[handler]
async fn users_emails_toggle_verified(
    Path((user_id, email_id)): Path<(String, i64)>,
    Authenticated(_user_info): Authenticated,
) -> Result<poem::Response, anyhow::Error> {
    let parsed_user_id = user_id
        .parse::<ulid::Ulid>()
        .map_err(|_| anyhow::anyhow!("Invalid user ID format"))?;

    let email_request = UserEmailToggleVerifiedRequest {
        user_id: parsed_user_id,
        email_id,
    };

    match request_with_retry::<UserDetailsResponse>(
        "user.admin.email_toggle_verified",
        email_request,
    )
    .await
    {
        Ok(response) => {
            // Find the toggled email and render just that row
            if let Some(updated_email) =
                response.user.emails.iter().find(|e| e.email_id == email_id)
            {
                let email_row_html = EmailRow {
                    user_id: user_id.clone(),
                    email: updated_email.clone(),
                }
                .render_once()
                .unwrap_or_default();

                // Render the updated user row with OOB swap using the data we already have
                let user_row_html = UserRow {
                    user: response.user.clone(),
                    oob: true,
                    oob_swap: "outerHTML".to_string(), // Use element's existing ID
                }
                .render_once()
                .unwrap_or_default();

                // Combine email row update and user row OOB update
                let combined_html = format!("{}\n{}", email_row_html, user_row_html);

                Ok(poem::Response::builder()
                    .content_type("text/html")
                    .body(combined_html))
            } else {
                Ok(error_response(
                    poem::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "",
                ))
            }
        }
        Err(e) => Ok(error_response(
            poem::http::StatusCode::INTERNAL_SERVER_ERROR,
            &e.to_string(),
        )),
    }
}

#[cfg(test)]
mod route_tests {
    use poem::{EndpointExt, http::StatusCode, middleware::CatchPanic, test::TestClient};

    const USER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    /// Routing happens before a handler runs, so METHOD_NOT_ALLOWED here is the
    /// route table's doing and nothing else's. The handlers themselves need a
    /// database and the auth middleware, so they panic under test; CatchPanic
    /// turns that into a 500, which still tells us the request was dispatched.
    async fn status_of(method: &str, path: String) -> StatusCode {
        let cli = TestClient::new(super::routes().with(CatchPanic::new()));
        let resp = match method {
            "GET" => cli.get(path).send().await,
            "POST" => cli.post(path).send().await,
            "PUT" => cli.put(path).send().await,
            "DELETE" => cli.delete(path).send().await,
            "PATCH" => cli.patch(path).send().await,
            other => panic!("unsupported method {other}"),
        };
        resp.0.status()
    }

    async fn assert_routed(method: &str, path: String) {
        let status = status_of(method, path.clone()).await;
        assert_ne!(
            status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{method} {path} is not routed"
        );
    }

    /// Emails are addressed by their row's id, not by the address itself. An
    /// address is a natural key rather than an identity: it changes when
    /// edited, and it is not reliably a URL path segment -- an empty one is no
    /// segment at all, which once left a row that could not be named, and so
    /// could not be deleted.
    #[tokio::test]
    async fn email_addresses_are_addressed_by_id() {
        assert_routed("POST", format!("/{USER}/emails")).await;
        assert_routed("PUT", format!("/{USER}/emails/1")).await;
        assert_routed("DELETE", format!("/{USER}/emails/1")).await;
        assert_routed("POST", format!("/{USER}/emails/1/toggle-verified")).await;
    }

    /// The shape that produced the bug: with the address in the path, an empty
    /// address left no segment at all, so the URL collapsed onto the collection
    /// and the delete never reached a handler. Whether that surfaces as 404 or
    /// 405 depends on where the tree is mounted; what matters is that it is not
    /// treated as a deletion.
    #[tokio::test]
    async fn a_member_url_with_no_identifier_does_not_delete() {
        let status = status_of("DELETE", format!("/{USER}/emails/")).await;
        assert!(
            status.is_client_error(),
            "expected a client error, got {status}"
        );
    }

    /// Updating a user is a PUT, which is what the edit-user modal sends. POST
    /// is accepted alongside it only because the edit *page* submits as a plain
    /// <form>, which a browser can only send as GET or POST.
    #[tokio::test]
    async fn a_user_is_updated_with_put_or_post() {
        assert_routed("PUT", format!("/{USER}")).await;
        assert_routed("POST", format!("/{USER}")).await;
    }

    /// The route table answers 405 rather than 404 for a method it does not
    /// have, which is what makes the assertions above meaningful.
    #[tokio::test]
    async fn an_unregistered_method_is_rejected_as_such() {
        assert_eq!(
            status_of("PATCH", format!("/{USER}")).await,
            StatusCode::METHOD_NOT_ALLOWED
        );
    }
}
