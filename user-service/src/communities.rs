use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::user::{
        CommunityCreateRequest, CommunityDeleteRequest, CommunityGetRequest, CommunityInfo,
        CommunityJoinRequest, CommunityLeaveRequest, CommunityListRequest, CommunityListResponse,
        CommunityMemberCheckRequest, CommunityMemberCheckResponse, CommunityMembersRequest,
        CommunityMembersResponse, CommunityMembership, CommunitySuccessResponse,
        CommunityUpdateRequest, UserCommunitiesRequest, UserCommunitiesResponse,
    },
    service::{TriggerEvent, decode_structured_message, log_activity, trigger_email},
    shared::db,
    timestring::TimeString,
    ulid::AsUlid,
};

struct CommunityRow {
    community_id: String,
    name: String,
    slug: String,
    subtitle: String,
    intro: String,
    description: String,
    image_url: String,
    partner_tag: String,
    topics: String,
    external_url: String,
    active: bool,
    featured: bool,
    exclusive: bool,
    sort_order: i32,
    member_count: i64,
}

impl CommunityRow {
    fn into_info(self) -> Result<CommunityInfo, Error> {
        Ok(CommunityInfo {
            community_id: self.community_id.as_ulid()?,
            name: self.name,
            slug: self.slug,
            subtitle: self.subtitle,
            intro: self.intro,
            description: self.description,
            image_url: self.image_url,
            partner_tag: self.partner_tag,
            topics: if self.topics.is_empty() {
                vec![]
            } else {
                self.topics
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect()
            },
            external_url: self.external_url,
            active: self.active,
            featured: self.featured,
            exclusive: self.exclusive,
            sort_order: self.sort_order,
            member_count: self.member_count,
        })
    }
}

pub async fn list(req: &Request) -> Result<CommunityListResponse, Error> {
    let message: CommunityListRequest = decode_structured_message(req.message.clone())?;

    let rows = if message.featured_only {
        sqlx::query_as!(
            CommunityRow,
            r#"
            SELECT c.community_id, c.name, c.slug, c.subtitle, c.intro, c.description,
                   c.image_url, c.partner_tag, c.topics, c.external_url,
                   c.active AS "active: bool", c.featured AS "featured: bool",
                   c.exclusive AS "exclusive: bool",
                   c.sort_order AS "sort_order: i32",
                   (SELECT COUNT(*) FROM community_memberships m WHERE m.community_id = c.community_id) AS "member_count!: i64"
            FROM communities c
            WHERE c.active = 1 AND c.featured = 1
            ORDER BY c.sort_order, c.name
            "#,
        )
        .fetch_all(db())
        .await?
    } else if message.active_only {
        sqlx::query_as!(
            CommunityRow,
            r#"
            SELECT c.community_id, c.name, c.slug, c.subtitle, c.intro, c.description,
                   c.image_url, c.partner_tag, c.topics, c.external_url,
                   c.active AS "active: bool", c.featured AS "featured: bool",
                   c.exclusive AS "exclusive: bool",
                   c.sort_order AS "sort_order: i32",
                   (SELECT COUNT(*) FROM community_memberships m WHERE m.community_id = c.community_id) AS "member_count!: i64"
            FROM communities c
            WHERE c.active = 1
            ORDER BY c.sort_order, c.name
            "#,
        )
        .fetch_all(db())
        .await?
    } else {
        sqlx::query_as!(
            CommunityRow,
            r#"
            SELECT c.community_id, c.name, c.slug, c.subtitle, c.intro, c.description,
                   c.image_url, c.partner_tag, c.topics, c.external_url,
                   c.active AS "active: bool", c.featured AS "featured: bool",
                   c.exclusive AS "exclusive: bool",
                   c.sort_order AS "sort_order: i32",
                   (SELECT COUNT(*) FROM community_memberships m WHERE m.community_id = c.community_id) AS "member_count!: i64"
            FROM communities c
            ORDER BY c.sort_order, c.name
            "#,
        )
        .fetch_all(db())
        .await?
    };

    let communities = rows
        .into_iter()
        .map(|r| r.into_info())
        .collect::<Result<Vec<_>, _>>()?;

    Ok(CommunityListResponse { communities })
}

/// Look up a community by slug.
///
/// Returns `None` when there is no such community, rather than an error, so
/// that callers can tell a missing community from a lookup that failed. They
/// are very different things: the first is a 404, the second a 500.
pub async fn get(req: &Request) -> Result<Option<CommunityInfo>, Error> {
    let message: CommunityGetRequest = decode_structured_message(req.message.clone())?;

    let row = sqlx::query_as!(
        CommunityRow,
        r#"
        SELECT c.community_id, c.name, c.slug, c.subtitle, c.intro, c.description,
               c.image_url, c.partner_tag, c.topics, c.external_url,
               c.active AS "active: bool", c.featured AS "featured: bool",
               c.exclusive AS "exclusive: bool",
               c.sort_order AS "sort_order: i32",
               (SELECT COUNT(*) FROM community_memberships m WHERE m.community_id = c.community_id) AS "member_count!: i64"
        FROM communities c
        WHERE c.slug = ?
        "#,
        message.slug,
    )
    .fetch_optional(db())
    .await?;

    row.map(CommunityRow::into_info).transpose()
}

pub async fn create(req: &Request) -> Result<CommunitySuccessResponse, Error> {
    let message: CommunityCreateRequest = decode_structured_message(req.message.clone())?;

    let community_id = ulid::Ulid::new().to_string();
    let topics = message.topics.join(",");

    sqlx::query!(
        r#"
        INSERT INTO communities (community_id, name, slug, subtitle, intro, description,
                                 image_url, partner_tag, topics, external_url,
                                 active, featured, exclusive, sort_order)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        community_id,
        message.name,
        message.slug,
        message.subtitle,
        message.intro,
        message.description,
        message.image_url,
        message.partner_tag,
        topics,
        message.external_url,
        message.active,
        message.featured,
        message.exclusive,
        message.sort_order,
    )
    .execute(db())
    .await?;

    Ok(CommunitySuccessResponse {
        success: true,
        message: "Community created successfully".to_string(),
    })
}

pub async fn update(req: &Request) -> Result<CommunitySuccessResponse, Error> {
    let message: CommunityUpdateRequest = decode_structured_message(req.message.clone())?;

    let community_id = message.community_id.to_string();
    let topics = message.topics.join(",");

    sqlx::query!(
        r#"
        UPDATE communities
        SET name = ?, slug = ?, subtitle = ?, intro = ?, description = ?,
            image_url = ?, partner_tag = ?, topics = ?, external_url = ?,
            active = ?, featured = ?, exclusive = ?, sort_order = ?
        WHERE community_id = ?
        "#,
        message.name,
        message.slug,
        message.subtitle,
        message.intro,
        message.description,
        message.image_url,
        message.partner_tag,
        topics,
        message.external_url,
        message.active,
        message.featured,
        message.exclusive,
        message.sort_order,
        community_id,
    )
    .execute(db())
    .await?;

    Ok(CommunitySuccessResponse {
        success: true,
        message: "Community updated successfully".to_string(),
    })
}

pub async fn delete(req: &Request) -> Result<CommunitySuccessResponse, Error> {
    let message: CommunityDeleteRequest = decode_structured_message(req.message.clone())?;

    let community_id = message.community_id.to_string();

    sqlx::query!(
        "DELETE FROM community_memberships WHERE community_id = ?",
        community_id,
    )
    .execute(db())
    .await?;

    sqlx::query!(
        "DELETE FROM communities WHERE community_id = ?",
        community_id,
    )
    .execute(db())
    .await?;

    Ok(CommunitySuccessResponse {
        success: true,
        message: "Community deleted successfully".to_string(),
    })
}

pub async fn join(req: &Request) -> Result<CommunitySuccessResponse, Error> {
    let message: CommunityJoinRequest = decode_structured_message(req.message.clone())?;

    let community_id = message.community_id.to_string();
    let user_id = message.user_id.to_string();
    let now = TimeString::now().to_string();

    sqlx::query!(
        r#"
        INSERT INTO community_memberships (community_id, user_id, joined_at)
        VALUES (?, ?, ?)
        ON CONFLICT(community_id, user_id) DO NOTHING
        "#,
        community_id,
        user_id,
        now,
    )
    .execute(db())
    .await?;

    let community_row = sqlx::query!(
        "SELECT name, slug FROM communities WHERE community_id = ?",
        community_id
    )
    .fetch_one(db())
    .await?;

    log_activity(
        None,
        "community-join",
        message.user_id,
        message.community_id,
        None,
    )
    .await
    .ok();
    let community_url = format!("/communities/{}", community_row.slug);
    trigger_email(
        TriggerEvent::CommunityJoin,
        message.user_id,
        Some(&[
            ("community_id".to_string(), message.community_id.to_string()),
            ("community_name".to_string(), community_row.name),
            ("community_url".to_string(), community_url),
        ]),
    )
    .await
    .ok();

    Ok(CommunitySuccessResponse {
        success: true,
        message: "Joined community successfully".to_string(),
    })
}

pub async fn leave(req: &Request) -> Result<CommunitySuccessResponse, Error> {
    let message: CommunityLeaveRequest = decode_structured_message(req.message.clone())?;

    let community_id = message.community_id.to_string();
    let user_id = message.user_id.to_string();

    sqlx::query!(
        "DELETE FROM community_memberships WHERE community_id = ? AND user_id = ?",
        community_id,
        user_id,
    )
    .execute(db())
    .await?;

    let community_row = sqlx::query!(
        "SELECT name, slug FROM communities WHERE community_id = ?",
        community_id
    )
    .fetch_one(db())
    .await?;

    log_activity(
        None,
        "community-leave",
        message.user_id,
        message.community_id,
        None,
    )
    .await
    .ok();
    let community_url = format!("/communities/{}", community_row.slug);
    trigger_email(
        TriggerEvent::CommunityLeave,
        message.user_id,
        Some(&[
            ("community_id".to_string(), message.community_id.to_string()),
            ("community_name".to_string(), community_row.name),
            ("community_url".to_string(), community_url),
        ]),
    )
    .await
    .ok();

    Ok(CommunitySuccessResponse {
        success: true,
        message: "Left community successfully".to_string(),
    })
}

pub async fn check_membership(req: &Request) -> Result<CommunityMemberCheckResponse, Error> {
    let message: CommunityMemberCheckRequest = decode_structured_message(req.message.clone())?;

    let community_id = message.community_id.to_string();
    let user_id = message.user_id.to_string();

    let result = sqlx::query_scalar!(
        r#"
        SELECT 1 AS "result: i32"
        FROM community_memberships
        WHERE community_id = ? AND user_id = ?
        LIMIT 1
        "#,
        community_id,
        user_id,
    )
    .fetch_optional(db())
    .await?;

    Ok(CommunityMemberCheckResponse {
        is_member: result.is_some(),
    })
}

pub async fn members(req: &Request) -> Result<CommunityMembersResponse, Error> {
    let message: CommunityMembersRequest = decode_structured_message(req.message.clone())?;

    let community_id = message.community_id.to_string();
    let page = message.page.unwrap_or(1);
    let limit = message.limit.unwrap_or(25);
    let offset = (page - 1) * limit;

    let rows = sqlx::query!(
        r#"
        SELECT u.user_id, u.handle, u.first_name, u.last_name, u.primary_email,
               cm.joined_at
        FROM community_memberships cm
        INNER JOIN users u ON u.user_id = cm.user_id
        WHERE cm.community_id = ?
        ORDER BY cm.joined_at DESC
        LIMIT ? OFFSET ?
        "#,
        community_id,
        limit,
        offset,
    )
    .fetch_all(db())
    .await?;

    let total_count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!: i64" FROM community_memberships WHERE community_id = ?"#,
        community_id,
    )
    .fetch_one(db())
    .await? as u64;

    let members = rows
        .into_iter()
        .map(|r| {
            Ok::<_, Error>(CommunityMembership {
                user_id: r.user_id.as_ulid()?,
                handle: r.handle,
                first_name: r.first_name,
                last_name: r.last_name,
                primary_email: r.primary_email,
                joined_at: r.joined_at,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(CommunityMembersResponse {
        members,
        total_count,
        page,
        limit,
    })
}

pub async fn user_communities(req: &Request) -> Result<UserCommunitiesResponse, Error> {
    let message: UserCommunitiesRequest = decode_structured_message(req.message.clone())?;

    let user_id = message.user_id.to_string();

    let rows = sqlx::query_as!(
        CommunityRow,
        r#"
        SELECT c.community_id, c.name, c.slug, c.subtitle, c.intro, c.description,
               c.image_url, c.partner_tag, c.topics, c.external_url,
               c.active AS "active: bool", c.featured AS "featured: bool",
               c.exclusive AS "exclusive: bool",
               c.sort_order AS "sort_order: i32",
               (SELECT COUNT(*) FROM community_memberships m WHERE m.community_id = c.community_id) AS "member_count!: i64"
        FROM communities c
        INNER JOIN community_memberships cm ON cm.community_id = c.community_id
        WHERE cm.user_id = ?
        ORDER BY c.sort_order, c.name
        "#,
        user_id,
    )
    .fetch_all(db())
    .await?;

    let communities = rows
        .into_iter()
        .map(|r| r.into_info())
        .collect::<Result<Vec<_>, _>>()?;

    Ok(UserCommunitiesResponse { communities })
}
