use crate::db;
use anyhow::Result;
use async_nats::service::Request;
use common::message::user::{RecipientSearchRequest, RecipientSearchResponse, RecipientSearchResult};
use common::service::decode_structured_message;

/// Search for recipients across users, user sets, and address sets
/// Supports email addresses, user handles/emails, and set names
/// Exact matches for email addresses and slugs are prioritized
pub async fn search(req: &Request) -> Result<RecipientSearchResponse> {
    let request: RecipientSearchRequest = decode_structured_message(req.message.clone())?;
    let query = request.query.trim();
    let limit = request.limit.unwrap_or(10) as i64;

    if query.is_empty() {
        return Ok(RecipientSearchResponse {
            results: vec![],
        });
    }

    let mut exact_matches = Vec::new();
    let mut partial_matches = Vec::new();
    let search_pattern = format!("%{}%", query);

    // If it looks like an email address, add it as an option
    if query.contains('@') {
        exact_matches.push(RecipientSearchResult::Email {
            email: query.to_string(),
        });
    }

    // Search for users by handle or primary email (limit to prevent overwhelming results)
    let user_rows = sqlx::query!(
        r#"
        SELECT user_id, handle, first_name, last_name, primary_email as email
        FROM users
        WHERE handle LIKE ? OR primary_email LIKE ?
        LIMIT ?
        "#,
        search_pattern,
        search_pattern,
        limit
    )
    .fetch_all(db())
    .await?;

    for row in user_rows {
        let display_name = if row.first_name.is_empty() && row.last_name.is_empty() {
            row.handle.clone()
        } else {
            format!("{} {}", row.first_name, row.last_name).trim().to_string()
        };

        let result = RecipientSearchResult::User {
            user_id: row.user_id.clone(),
            display_name,
            email: row.email.clone(),
        };

        // Check if this is an exact match on email
        if row.email.eq_ignore_ascii_case(query) {
            exact_matches.push(result);
        } else {
            partial_matches.push(result);
        }
    }

    // Search for user sets by name or slug
    let user_set_rows = sqlx::query!(
        r#"
        SELECT slug, name
        FROM user_sets
        WHERE name LIKE ? OR slug LIKE ?
        LIMIT ?
        "#,
        search_pattern,
        search_pattern,
        limit
    )
    .fetch_all(db())
    .await?;

    for row in user_set_rows {
        let result = RecipientSearchResult::UserSet {
            slug: row.slug.clone(),
            name: row.name.clone(),
        };

        // Check if this is an exact match on slug
        if row.slug.eq_ignore_ascii_case(query) {
            exact_matches.push(result);
        } else {
            partial_matches.push(result);
        }
    }

    // Search for address sets by name or slug
    let address_set_rows = sqlx::query!(
        r#"
        SELECT slug, name
        FROM address_sets
        WHERE name LIKE ? OR slug LIKE ?
        LIMIT ?
        "#,
        search_pattern,
        search_pattern,
        limit
    )
    .fetch_all(db())
    .await?;

    for row in address_set_rows {
        let result = RecipientSearchResult::AddressSet {
            slug: row.slug.clone(),
            name: row.name.clone(),
        };

        // Check if this is an exact match on slug
        if row.slug.eq_ignore_ascii_case(query) {
            exact_matches.push(result);
        } else {
            partial_matches.push(result);
        }
    }

    // Combine exact matches first, then partial matches
    let mut results = exact_matches;
    results.extend(partial_matches);

    // Limit total results
    results.truncate(limit as usize);

    Ok(RecipientSearchResponse { results })
}
