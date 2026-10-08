//! Who may run an exchange.
//!
//! An exchange's people are kept as user-service permissions on the
//! exchange's ULID: one owner, and any number of staff. Both may run the
//! exchange; the owner is the account it belongs to, as in orrery, where it
//! is `Exchange.owner` beside `Exchange.staff`. SciStarter's own staff and
//! superusers may run every exchange without a permission of their own.

use anyhow::Error;
use common::message::user::{
    BasicInfo, PermissionCheckRequest, PermissionCheckResponse, PermissionGrantRequest,
    PermissionListByResourceRequest, PermissionListByResourceResponse, PermissionRevokeRequest,
};
use common::service::request;
use ulid::Ulid;

const RESOURCE_TYPE: &str = "exchange";

/// The part a person plays in running an exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeRole {
    Owner,
    Staff,
}

impl ExchangeRole {
    /// The `permission` value user-service stores for the role.
    pub fn permission(self) -> &'static str {
        match self {
            ExchangeRole::Owner => "own",
            ExchangeRole::Staff => "manage",
        }
    }

    /// The role a stored `permission` value names. Anything unrecognised is
    /// read as staff, the lesser role, since holding the permission at all
    /// is what lets a person run the exchange.
    pub fn from_permission(permission: &str) -> ExchangeRole {
        match permission {
            "own" => ExchangeRole::Owner,
            _ => ExchangeRole::Staff,
        }
    }
}

/// Whether `user` may run the exchange.
pub async fn may_manage(exchange_id: Ulid, user: Option<&BasicInfo>) -> Result<bool, Error> {
    let Some(user) = user else {
        return Ok(false);
    };

    if user.is_staff || user.is_superuser {
        return Ok(true);
    }

    let response = request::<PermissionCheckResponse>(
        "user.permissions.check",
        PermissionCheckRequest {
            user_id: user.user_id.to_string(),
            resource_type: RESOURCE_TYPE.to_string(),
            resource_id: exchange_id.to_string(),
        },
    )
    .await?;

    Ok(response.has_permission)
}

/// Everyone who holds a role on the exchange.
pub async fn people(exchange_id: Ulid) -> Result<Vec<(Ulid, ExchangeRole)>, Error> {
    let response = request::<PermissionListByResourceResponse>(
        "user.permissions.list_by_resource",
        PermissionListByResourceRequest {
            resource_type: RESOURCE_TYPE.to_string(),
            resource_id: exchange_id.to_string(),
        },
    )
    .await?;

    Ok(response
        .permissions
        .into_iter()
        .filter_map(|permission| {
            Ulid::from_string(&permission.user_id).ok().map(|user_id| {
                (
                    user_id,
                    ExchangeRole::from_permission(&permission.permission),
                )
            })
        })
        .collect())
}

/// Give `user_id` a role on the exchange.
///
/// A person holds one role at a time, so whatever they held before is
/// replaced.
pub async fn grant(
    exchange_id: Ulid,
    user_id: Ulid,
    role: ExchangeRole,
    granted_by: &str,
) -> Result<(), Error> {
    revoke(exchange_id, user_id).await?;

    request::<()>(
        "user.permissions.grant",
        PermissionGrantRequest {
            user_id: user_id.to_string(),
            resource_type: RESOURCE_TYPE.to_string(),
            resource_id: exchange_id.to_string(),
            permission: role.permission().to_string(),
            granted_by: granted_by.to_string(),
        },
    )
    .await
}

/// Take away whatever role `user_id` holds on the exchange.
pub async fn revoke(exchange_id: Ulid, user_id: Ulid) -> Result<(), Error> {
    revoke_on(&exchange_id.to_string(), user_id).await
}

/// Revoke a role held on `resource_id`, which is the exchange's ULID except
/// while clearing grants made under its `path_id` (see [`set_people`]).
async fn revoke_on(resource_id: &str, user_id: Ulid) -> Result<(), Error> {
    request::<()>(
        "user.permissions.revoke",
        PermissionRevokeRequest {
            user_id: user_id.to_string(),
            resource_type: RESOURCE_TYPE.to_string(),
            resource_id: resource_id.to_string(),
        },
    )
    .await
}

/// Make the exchange's people exactly `owner` and `staff`.
///
/// Grants were once made against the exchange's `path_id` rather than its
/// ULID, which nothing now checks, so any left there are cleared too.
pub async fn set_people(
    exchange_id: Ulid,
    path_id: i64,
    owner: Option<Ulid>,
    staff: &[Ulid],
    granted_by: &str,
) -> Result<(), Error> {
    let mut wanted: Vec<(Ulid, ExchangeRole)> = staff
        .iter()
        .filter(|user_id| Some(**user_id) != owner)
        .map(|user_id| (*user_id, ExchangeRole::Staff))
        .collect();
    if let Some(owner) = owner {
        wanted.push((owner, ExchangeRole::Owner));
    }

    let held = people(exchange_id).await?;

    for (user_id, _) in held.iter() {
        if !wanted.iter().any(|(wanted_id, _)| wanted_id == user_id) {
            revoke(exchange_id, *user_id).await?;
        }
    }

    for (user_id, role) in wanted.iter() {
        if !held.contains(&(*user_id, *role)) {
            grant(exchange_id, *user_id, *role, granted_by).await?;
        }
    }

    let stale = request::<PermissionListByResourceResponse>(
        "user.permissions.list_by_resource",
        PermissionListByResourceRequest {
            resource_type: RESOURCE_TYPE.to_string(),
            resource_id: path_id.to_string(),
        },
    )
    .await?;

    for permission in stale.permissions {
        if let Ok(user_id) = Ulid::from_string(&permission.user_id) {
            revoke_on(&path_id.to_string(), user_id).await?;
        }
    }

    Ok(())
}
