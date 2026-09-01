//! The few projects this service has to recognize by the name they are
//! written down as.
//!
//! A participation record names its project by id, and the entity tables live
//! in the frontend's database, so the ids are resolved from the slugs once, on
//! first use, and remembered. A project id never changes, and there are a
//! handful of them, so this costs one round of requests over the life of the
//! process rather than a lookup per contribution.
//!
//! A failed resolution is not remembered: `get_or_try_init` leaves the cell
//! empty, and the next contribution tries again. A slug no project carries is
//! logged and left out, so the rest still resolve -- refusing to recognize any
//! of them because one was renamed would be the worse failure.

use std::collections::HashMap;

use anyhow::Error;
use common::message::dashboard::EntitySummary;
use common::service::request;
use tokio::sync::OnceCell;

/// The projects a participant works through as steps of the Foundations of
/// Citizen Science module. Any two of the three finish its project
/// requirement.
pub const SQUIRREL: &str = "project-squirrel";
pub const STALL: &str = "stall-catchers-by-eyesonalz";
pub const DEBRIS: &str = "marine-debris-tracker";

/// The project page standing for the Foundations module itself. A
/// contribution against it means somebody finished the training, which is not
/// the same kind of thing as taking part in a project.
pub const FOUNDATIONS_MODULE: &str = "foundations-of-citizen-science-training-module";

/// Excluded alongside the training module from the first-contribution mail,
/// as it is in orrery, and for as long as it is there.
pub const ISEECHANGE: &str = "iseechange";

const KNOWN: [&str; 5] = [SQUIRREL, STALL, DEBRIS, FOUNDATIONS_MODULE, ISEECHANGE];

/// Project id to the slug it is written down as, for [`KNOWN`] only.
static SLUGS: OnceCell<HashMap<String, &'static str>> = OnceCell::const_new();

async fn slugs() -> Result<&'static HashMap<String, &'static str>, Error> {
    SLUGS
        .get_or_try_init(|| async {
            let mut slugs = HashMap::with_capacity(KNOWN.len());

            for slug in KNOWN {
                let summary: Option<EntitySummary> =
                    request("frontend.entity.summary_by_slug", slug.to_string()).await?;

                match summary {
                    Some(summary) => {
                        slugs.insert(summary.id.to_string(), slug);
                    }
                    None => eprintln!(
                        "No project carries the slug `{slug}`, so nothing this service \
                         decides by that name will apply to it"
                    ),
                }
            }

            Ok(slugs)
        })
        .await
}

/// The slug `project_id` is written down as, when it is one of the few this
/// service knows by name. `None` for every other project, which is almost all
/// of them.
pub async fn slug_of(project_id: &str) -> Result<Option<&'static str>, Error> {
    Ok(slugs().await?.get(project_id).copied())
}
