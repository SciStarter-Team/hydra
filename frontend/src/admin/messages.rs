//! Staff review queue for direct messages. Every in-app message is held until
//! a staff member approves it; this page lists the pending messages and lets
//! staff approve (deliver) or reject them. Copies of one bulk send share a
//! `batch` id and are grouped into a single row so staff act on the whole
//! batch at once; ordinary one-recipient messages are reviewed individually —
//! both in this one page, as on orrery. Modeled on the ambassador contact
//! queue (`admin::ambassadors`). Nested under the already-`StaffOnly`
//! `admin::routes()`.

use crate::component::{AdminLayout, Component};
use crate::middleware::authorization::Authenticated;
use common::message::forum::{ReviewBatchDecision, ReviewMessageDecision, ReviewMessageRow};
use common::service::request;
use poem::{
    Endpoint, Route, get, handler, post,
    web::{Form, Path},
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

/// One entry in the review queue: either a single message, or a whole bulk
/// send collapsed into one reviewable unit.
#[derive(Clone)]
pub enum ReviewGroup {
    Single(ReviewMessageRow),
    Batch(BatchGroup),
}

/// A bulk send (one message to many recipients) presented as a single
/// reviewable unit. Subject/body/sender/when come from the first copy (they
/// are identical across the batch); `recipients` lists every addressee.
#[derive(Clone)]
pub struct BatchGroup {
    pub batch: Ulid,
    pub sender: String,
    pub subject: String,
    pub body: String,
    pub when: String,
    pub count: usize,
    pub recipients: Vec<String>,
}

/// Collapse the flat pending list into review groups: every row sharing a
/// `batch` becomes one `Batch`, preserving first-appearance order; rows with
/// no batch stay individual `Single`s.
fn group_rows(rows: Vec<ReviewMessageRow>) -> Vec<ReviewGroup> {
    use std::collections::HashMap;

    let mut groups: Vec<ReviewGroup> = Vec::new();
    let mut batch_index: HashMap<Ulid, usize> = HashMap::new();

    for row in rows {
        match row.batch {
            None => groups.push(ReviewGroup::Single(row)),
            Some(batch) => {
                if let Some(&idx) = batch_index.get(&batch) {
                    if let ReviewGroup::Batch(b) = &mut groups[idx] {
                        b.count += 1;
                        b.recipients.push(row.recipient);
                    }
                } else {
                    batch_index.insert(batch, groups.len());
                    groups.push(ReviewGroup::Batch(BatchGroup {
                        batch,
                        sender: row.sender,
                        subject: row.subject,
                        body: row.body,
                        when: row.when,
                        count: 1,
                        recipients: vec![row.recipient],
                    }));
                }
            }
        }
    }

    groups
}

#[derive(TemplateOnce)]
#[template(path = "admin/message_review.stpl.html")]
struct MessageReviewPage {
    groups: Vec<ReviewGroup>,
}

/// One pending single-message `<tr>`, rendered inside the page.
#[derive(TemplateOnce)]
#[template(path = "admin/message_review_row.stpl.html")]
pub struct ReviewRow {
    pub row: ReviewMessageRow,
}

/// One pending-batch `<tr>` (approve/reject the whole bulk send at once).
#[derive(TemplateOnce)]
#[template(path = "admin/message_review_batch_row.stpl.html")]
pub struct BatchReviewRow {
    pub batch: BatchGroup,
}

pub fn routes() -> impl Endpoint {
    Route::new()
        .at("/", get(index))
        .at("/review/:id", post(review))
        .at("/review-batch/:batch", post(review_batch))
}

#[handler]
async fn index(Authenticated(user_info): Authenticated) -> Result<poem::Response, anyhow::Error> {
    let rows: Vec<ReviewMessageRow> = request("forum.messaging.pending_list", ()).await?;
    let page = MessageReviewPage {
        groups: group_rows(rows),
    };
    let prepared = AdminLayout::new("Message Review", page)
        .with_sidebar_active("messages")
        .with_breadcrumb("Message Review", Some("/-admin/messages"))
        .prepare(user_info)
        .await?;
    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(prepared.render_once()?))
}

#[derive(Deserialize)]
struct ReviewForm {
    /// "accept" delivers the message(s); anything else rejects them.
    decision: String,
}

/// The `<tr>` confirmation that replaces a reviewed row/batch after a decision.
fn decision_row(accept: bool) -> String {
    let label = if accept {
        "Approved &amp; delivered"
    } else {
        "Rejected"
    };
    format!(
        "<tr class=\"border-t border-gray-100\">\
           <td colspan=\"5\" class=\"px-2 py-2 text-gray-500\">{}</td>\
         </tr>",
        label
    )
}

#[handler]
async fn review(
    Path(id): Path<String>,
    Authenticated(_user_info): Authenticated,
    Form(form): Form<ReviewForm>,
) -> Result<poem::Response, anyhow::Error> {
    let post_id = Ulid::from_string(&id)?;
    let accept = form.decision == "accept";
    let _: () = request(
        "forum.messaging.review",
        ReviewMessageDecision { post_id, accept },
    )
    .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(decision_row(accept)))
}

#[handler]
async fn review_batch(
    Path(batch): Path<String>,
    Authenticated(_user_info): Authenticated,
    Form(form): Form<ReviewForm>,
) -> Result<poem::Response, anyhow::Error> {
    let batch = Ulid::from_string(&batch)?;
    let accept = form.decision == "accept";
    let _: () = request(
        "forum.messaging.review_batch",
        ReviewBatchDecision { batch, accept },
    )
    .await?;

    Ok(poem::Response::builder()
        .content_type("text/html")
        .body(decision_row(accept)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(recipient: &str, batch: Option<Ulid>) -> ReviewMessageRow {
        ReviewMessageRow {
            post_id: Ulid::new(),
            sender: "Project".into(),
            recipient: recipient.into(),
            subject: "Hi".into(),
            body: "Body".into(),
            when: "now".into(),
            batch,
        }
    }

    fn single_recipient(group: &ReviewGroup) -> &str {
        match group {
            ReviewGroup::Single(r) => &r.recipient,
            ReviewGroup::Batch(_) => panic!("expected a single, got a batch"),
        }
    }

    #[test]
    fn empty_list_yields_no_groups() {
        assert!(group_rows(vec![]).is_empty());
    }

    #[test]
    fn unbatched_rows_stay_individual_and_ordered() {
        let groups = group_rows(vec![row("a", None), row("b", None)]);
        assert_eq!(groups.len(), 2);
        assert_eq!(single_recipient(&groups[0]), "a");
        assert_eq!(single_recipient(&groups[1]), "b");
    }

    #[test]
    fn copies_of_one_batch_collapse_into_a_single_group() {
        let batch = Ulid::new();
        let groups = group_rows(vec![
            row("a", Some(batch)),
            row("b", Some(batch)),
            row("c", Some(batch)),
        ]);
        assert_eq!(groups.len(), 1);
        match &groups[0] {
            ReviewGroup::Batch(b) => {
                assert_eq!(b.batch, batch);
                assert_eq!(b.count, 3);
                assert_eq!(b.recipients, vec!["a", "b", "c"]);
            }
            _ => panic!("expected a batch group"),
        }
    }

    #[test]
    fn batches_and_singles_interleave_preserving_first_appearance_order() {
        let batch = Ulid::new();
        let groups = group_rows(vec![
            row("solo1", None),
            row("bulk1", Some(batch)),
            row("solo2", None),
            row("bulk2", Some(batch)),
        ]);
        // solo1, then the batch (merged where it first appeared), then solo2.
        assert_eq!(groups.len(), 3);
        assert_eq!(single_recipient(&groups[0]), "solo1");
        match &groups[1] {
            ReviewGroup::Batch(b) => {
                assert_eq!(b.count, 2);
                assert_eq!(b.recipients, vec!["bulk1", "bulk2"]);
            }
            _ => panic!("expected batch group at index 1"),
        }
        assert_eq!(single_recipient(&groups[2]), "solo2");
    }

    #[test]
    fn distinct_batches_stay_separate() {
        let a = Ulid::new();
        let b = Ulid::new();
        let groups = group_rows(vec![row("x", Some(a)), row("y", Some(b))]);
        assert_eq!(groups.len(), 2);
        assert!(matches!(groups[0], ReviewGroup::Batch(_)));
        assert!(matches!(groups[1], ReviewGroup::Batch(_)));
    }
}
