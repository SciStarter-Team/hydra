//! Who may edit a project's or an event's page, as orrery's `can_write`
//! decides: SciStarter's staff and superusers, the page's creator and its
//! editors, and the owner and staff of the exchange it belongs to.
//!
//! Everything that lets someone edit a page, see it unpublished, write to it
//! through the partner API, or message its participants asks here, so they
//! all agree.

use anyhow::Error;
use common::{message::user::BasicInfo, shared::db};
use ulid::Ulid;

/// A page that can be edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Project(Ulid),
    Event(Ulid),
}

/// Someone who might edit a page.
#[derive(Debug, Clone, Copy)]
pub struct Editor {
    pub user_id: Ulid,
    /// SciStarter staff or a superuser.
    pub is_staff: bool,
}

impl From<&BasicInfo> for Editor {
    fn from(user: &BasicInfo) -> Editor {
        Editor {
            user_id: user.user_id,
            is_staff: user.is_staff || user.is_superuser,
        }
    }
}

/// Whether `editor` may edit `page`. A page that does not exist may not be
/// edited.
pub async fn may_edit(page: Page, editor: Editor) -> Result<bool, Error> {
    if editor.is_staff {
        return Ok(true);
    }

    let user_id = editor.user_id.to_string();

    let found = match page {
        Page::Project(project_id) => {
            let project_id = project_id.to_string();
            sqlx::query!(
                r#"SELECT (p.page_creator_id = ?1 OR EXISTS (
                       SELECT 1 FROM project_page_editors e
                       WHERE e.project_id = p.project_id AND e.user_id = ?1
                   )) AS "named!: bool",
                   p.exchange
                   FROM projects p WHERE p.project_id = ?2"#,
                user_id,
                project_id,
            )
            .fetch_optional(db())
            .await?
            .map(|row| (row.named, row.exchange))
        }
        Page::Event(event_id) => {
            let event_id = event_id.to_string();
            sqlx::query!(
                r#"SELECT (v.page_creator_id = ?1 OR EXISTS (
                       SELECT 1 FROM event_page_editors e
                       WHERE e.event_id = v.event_id AND e.user_id = ?1
                   )) AS "named!: bool",
                   v.exchange
                   FROM events v WHERE v.event_id = ?2"#,
                user_id,
                event_id,
            )
            .fetch_optional(db())
            .await?
            .map(|row| (row.named, row.exchange))
        }
    };

    let Some((named, exchange)) = found else {
        return Ok(false);
    };

    if named {
        return Ok(true);
    }

    match exchange.and_then(|exchange| Ulid::from_string(&exchange).ok()) {
        Some(exchange_id) => crate::exchange::access::holds_role(exchange_id, editor.user_id).await,
        None => Ok(false),
    }
}
