//! Signup tags whose cohort is credited with a training outright -- see
//! [`SignupTagBadge`] for why such a thing exists.
//!
//! Staff keep the grants in orrery's admin, and the signup-tag-grant sync
//! copies them here; staff may also make grants of their own in hydra's admin.
//! A grant is consulted at the two places a signup-context tag reaches an
//! account: `create_account`, for every signup lane, and `apply_tag` under
//! `TagSource::Signup`, for every login lane.

use std::collections::HashSet;

use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::user::{
        SIGNUP_TAG_GRANT_NEXT_PATH_LENGTH, SIGNUP_TAG_GRANT_TAG_LENGTH, SignupTagBadge,
        SignupTagGrant, SignupTagGrantAdd, SignupTagGrantAdded, SignupTagGrantDelete,
        normalize_tag,
    },
    redirect::is_local_next,
    service::decode_structured_message,
    shared::db,
    timestring::TimeString,
    transaction::Immediate,
};
use jiff::Timestamp;
use sqlx::SqlitePool;

/// The badges an account carrying `tags` from its signup context is credited
/// with. Tags are matched in their normalized spelling, as stored.
pub async fn badges_for<S: AsRef<str>>(
    pool: &SqlitePool,
    tags: &[S],
) -> Result<HashSet<SignupTagBadge>, Error> {
    let tags: Vec<String> = tags
        .iter()
        .map(|tag| normalize_tag(tag.as_ref()))
        .filter(|tag| !tag.is_empty())
        .collect();

    if tags.is_empty() {
        return Ok(HashSet::new());
    }

    let tags = serde_json::to_string(&tags)?;

    let badges = sqlx::query_scalar!(
        r#"SELECT DISTINCT "badge" FROM "signup_tag_grants"
           WHERE "tag" IN (SELECT "value" FROM json_each(?))"#,
        tags,
    )
    .fetch_all(pool)
    .await?;

    Ok(badges
        .into_iter()
        .filter_map(|badge| {
            let parsed = SignupTagBadge::parse(&badge);

            if parsed.is_none() {
                eprintln!("A signup tag grant names `{badge}`, which is no badge known here");
            }

            parsed
        })
        .collect())
}

/// Credit `user_id` with every training its signup-context `tags` grant.
/// Each training's credit is idempotent, so this is safe for an account part
/// way through or already finished.
pub async fn credit<S: AsRef<str>>(user_id: &str, tags: &[S]) -> Result<(), Error> {
    for badge in badges_for(db(), tags).await? {
        match badge {
            SignupTagBadge::Foundations => {
                crate::workflow::credit_foundations_cohort(user_id).await?;
            }
        }
    }

    Ok(())
}

pub async fn list(_req: &Request) -> Result<Vec<SignupTagGrant>, Error> {
    list_grants(db()).await
}

async fn list_grants(pool: &SqlitePool) -> Result<Vec<SignupTagGrant>, Error> {
    let rows = sqlx::query!(
        r#"SELECT "id", "tag", "badge", "next_path", "created", "orrery_id"
           FROM "signup_tag_grants"
           ORDER BY "tag", "badge", "id""#
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(SignupTagGrant {
                id: row.id,
                tag: row.tag,
                badge: SignupTagBadge::parse(&row.badge)?,
                next_path: row.next_path,
                created: row.created,
                from_orrery: row.orrery_id.is_some(),
            })
        })
        .collect())
}

pub async fn add(req: &Request) -> Result<SignupTagGrantAdded, Error> {
    let add: SignupTagGrantAdd = decode_structured_message(req.message.clone())?;
    add_grant(db(), &add.tag, add.badge, &add.next_path).await
}

/// Grant `badge` to the cohort tagged `tag`, once normalized, unless it is
/// granted already -- by a grant made here or one copied from orrery. The
/// link handing it out sends people on to `next_path`, once trimmed, if that
/// is not empty.
async fn add_grant(
    pool: &SqlitePool,
    tag: &str,
    badge: SignupTagBadge,
    next_path: &str,
) -> Result<SignupTagGrantAdded, Error> {
    let tag = normalize_tag(tag);

    if tag.is_empty() {
        return Ok(SignupTagGrantAdded::EmptyTag);
    }

    if tag.chars().count() > SIGNUP_TAG_GRANT_TAG_LENGTH {
        return Ok(SignupTagGrantAdded::TagTooLong);
    }

    // Login refuses a `next` that is not a path here, so a link carrying one
    // would silently drop it; refused here instead, where it can be corrected.
    let next_path = next_path.trim();

    if !next_path.is_empty() && !is_local_next(next_path) {
        return Ok(SignupTagGrantAdded::NextPathNotLocal);
    }

    if next_path.chars().count() > SIGNUP_TAG_GRANT_NEXT_PATH_LENGTH {
        return Ok(SignupTagGrantAdded::NextPathTooLong);
    }

    let badge_name = badge.as_str();
    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;

    let granted = sqlx::query_scalar!(
        r#"SELECT EXISTS (
             SELECT 1 FROM "signup_tag_grants" WHERE "tag" = ? AND "badge" = ?
           ) AS "granted: bool""#,
        tag,
        badge_name,
    )
    .fetch_one(&mut tx)
    .await?;

    if granted {
        return Ok(SignupTagGrantAdded::AlreadyGranted(tag));
    }

    let created = TimeString::now().to_string();

    let id = sqlx::query_scalar!(
        r#"INSERT INTO "signup_tag_grants" ("tag", "badge", "next_path", "created")
           VALUES (?, ?, ?, ?)
           RETURNING "id""#,
        tag,
        badge_name,
        next_path,
        created,
    )
    .fetch_one(&mut tx)
    .await?;

    tx.commit().await?;

    Ok(SignupTagGrantAdded::Added(SignupTagGrant {
        id,
        tag,
        badge,
        next_path: next_path.to_string(),
        created,
        from_orrery: false,
    }))
}

pub async fn delete(req: &Request) -> Result<(), Error> {
    let delete: SignupTagGrantDelete = decode_structured_message(req.message.clone())?;
    delete_grant(db(), delete.id).await
}

/// Delete a grant made here. A copy of orrery's is left alone: the next sync
/// would only bring it back.
async fn delete_grant(pool: &SqlitePool, id: i64) -> Result<(), Error> {
    sqlx::query!(
        r#"DELETE FROM "signup_tag_grants" WHERE "id" = ? AND "orrery_id" IS NULL"#,
        id,
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// One of orrery's signup tag grants, as `/staff/sync/signup_tag_grants`
/// lists it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OrreryGrant {
    pub id: i64,
    pub tag: String,
    pub badge: String,
    pub next_path: String,
    pub created: String,
}

/// Make the grants copied from orrery match `grants`, the whole of orrery's
/// set, in one transaction: each is added or brought up to date, and a copy
/// whose original is gone is deleted. Grants made here are left alone.
///
/// A grant naming a badge not known here is skipped, and arrives with the
/// first snapshot after hydra learns it. A path to send people on to that is
/// not a path here is dropped: orrery refuses one, so it can only be damage.
pub async fn mirror_orrery(grants: &[OrreryGrant]) -> Result<(), Error> {
    mirror(db(), grants).await
}

async fn mirror(pool: &SqlitePool, grants: &[OrreryGrant]) -> Result<(), Error> {
    let mut tx = Immediate::begin_with_retry(pool, None, None).await?;
    let mut kept = Vec::with_capacity(grants.len());

    for g in grants {
        let Some(badge) = SignupTagBadge::parse(&g.badge) else {
            eprintln!(
                "Skipping orrery's signup tag grant {} for `{}`: no badge `{}` here",
                g.id, g.tag, g.badge
            );
            continue;
        };

        let badge = badge.as_str();
        let tag = normalize_tag(&g.tag);
        let next_path = match g.next_path.trim() {
            "" => "",
            path if is_local_next(path) => path,
            path => {
                eprintln!(
                    "Dropping the path `{path}` from orrery's signup tag grant {}: not a path here",
                    g.id
                );
                ""
            }
        };
        let created = g
            .created
            .parse::<Timestamp>()
            .map(TimeString::from)
            .unwrap_or_else(|_| TimeString::now())
            .to_string();

        sqlx::query!(
            r#"INSERT INTO "signup_tag_grants" ("tag", "badge", "next_path", "created", "orrery_id")
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT ("orrery_id") DO UPDATE SET
                 "tag" = excluded."tag",
                 "badge" = excluded."badge",
                 "next_path" = excluded."next_path",
                 "created" = excluded."created""#,
            tag,
            badge,
            next_path,
            created,
            g.id,
        )
        .execute(&mut tx)
        .await?;

        kept.push(g.id);
    }

    let kept = serde_json::to_string(&kept)?;

    sqlx::query!(
        r#"DELETE FROM "signup_tag_grants"
           WHERE "orrery_id" IS NOT NULL
             AND "orrery_id" NOT IN (SELECT "value" FROM json_each(?))"#,
        kept,
    )
    .execute(&mut tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::{check, let_assert};

    async fn fixture() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");

        for migration in [
            include_str!("../migrations/20261030235425_signup_tag_grants.up.sql"),
            include_str!("../migrations/20261031002010_signup_tag_grant_next_path.up.sql"),
        ] {
            sqlx::raw_sql(migration)
                .execute(&pool)
                .await
                .expect("schema");
        }

        pool
    }

    fn orrery(id: i64, tag: &str) -> OrreryGrant {
        OrreryGrant {
            id,
            tag: tag.to_string(),
            badge: "foundations".to_string(),
            next_path: String::new(),
            created: "2026-10-30T23:46:00.123456Z".to_string(),
        }
    }

    async fn tags(pool: &SqlitePool) -> Vec<(String, bool)> {
        list_grants(pool)
            .await
            .unwrap()
            .into_iter()
            .map(|g| (g.tag, g.from_orrery))
            .collect()
    }

    #[tokio::test]
    async fn a_tag_credits_in_any_spelling_its_partner_wrote_it_in() {
        let pool = fixture().await;
        mirror(&pool, &[orrery(1, "ldpl")]).await.unwrap();

        for spelling in ["ldpl", "LDPL", " #LDPL "] {
            check!(
                badges_for(&pool, &["some-partner", spelling])
                    .await
                    .unwrap()
                    == HashSet::from([SignupTagBadge::Foundations])
            );
        }
    }

    #[tokio::test]
    async fn an_ordinary_signup_is_not_credited() {
        let pool = fixture().await;
        mirror(&pool, &[orrery(1, "ldpl")]).await.unwrap();

        check!(
            badges_for(&pool, &["some-partner", "ldpls"])
                .await
                .unwrap()
                .is_empty()
        );
        check!(badges_for::<&str>(&pool, &[]).await.unwrap().is_empty());
        check!(badges_for(&pool, &[" # "]).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_mirror_follows_orrerys_set_and_leaves_hydras_grants_alone() {
        let pool = fixture().await;
        add_grant(&pool, "Webinar", SignupTagBadge::Foundations, "")
            .await
            .unwrap();

        mirror(&pool, &[orrery(1, "azla"), orrery(2, "call")])
            .await
            .unwrap();
        check!(
            tags(&pool).await
                == vec![
                    ("azla".to_string(), true),
                    ("call".to_string(), true),
                    ("webinar".to_string(), false),
                ]
        );

        // A grant renamed on orrery is renamed here, and one deleted there is
        // deleted here.
        mirror(&pool, &[orrery(2, "CALL-2026")]).await.unwrap();
        check!(
            tags(&pool).await
                == vec![
                    ("call-2026".to_string(), true),
                    ("webinar".to_string(), false),
                ]
        );

        mirror(&pool, &[]).await.unwrap();
        check!(tags(&pool).await == vec![("webinar".to_string(), false)]);
    }

    #[tokio::test]
    async fn a_grant_naming_an_unknown_badge_is_skipped() {
        let pool = fixture().await;
        let mut unknown = orrery(1, "azla");
        unknown.badge = "k12".to_string();

        mirror(&pool, &[unknown, orrery(2, "call")]).await.unwrap();

        check!(tags(&pool).await == vec![("call".to_string(), true)]);
    }

    #[tokio::test]
    async fn a_pair_already_granted_is_refused_whichever_side_made_it() {
        let pool = fixture().await;
        mirror(&pool, &[orrery(1, "azla")]).await.unwrap();

        let added = add_grant(&pool, "#AZLA", SignupTagBadge::Foundations, "")
            .await
            .unwrap();
        let_assert!(SignupTagGrantAdded::AlreadyGranted(tag) = added);
        check!(tag == "azla");

        add_grant(&pool, "call", SignupTagBadge::Foundations, "")
            .await
            .unwrap();
        let added = add_grant(&pool, "Call", SignupTagBadge::Foundations, "")
            .await
            .unwrap();
        check!(let SignupTagGrantAdded::AlreadyGranted(_) = added);
    }

    #[tokio::test]
    async fn a_tag_must_be_more_than_punctuation_and_no_longer_than_orrerys() {
        let pool = fixture().await;

        let added = add_grant(&pool, " # ", SignupTagBadge::Foundations, "")
            .await
            .unwrap();
        check!(let SignupTagGrantAdded::EmptyTag = added);

        let long = "x".repeat(SIGNUP_TAG_GRANT_TAG_LENGTH + 1);
        let added = add_grant(&pool, &long, SignupTagBadge::Foundations, "")
            .await
            .unwrap();
        check!(let SignupTagGrantAdded::TagTooLong = added);

        check!(tags(&pool).await.is_empty());
    }

    #[tokio::test]
    async fn only_a_grant_made_here_can_be_deleted_here() {
        let pool = fixture().await;
        mirror(&pool, &[orrery(1, "azla")]).await.unwrap();
        let_assert!(
            SignupTagGrantAdded::Added(made_here) =
                add_grant(&pool, "call", SignupTagBadge::Foundations, "")
                    .await
                    .unwrap()
        );

        for grant in list_grants(&pool).await.unwrap() {
            delete_grant(&pool, grant.id).await.unwrap();
        }

        check!(tags(&pool).await == vec![("azla".to_string(), true)]);
        check!(made_here.tag == "call");
    }

    #[tokio::test]
    async fn a_grant_keeps_where_its_link_sends_people() {
        let pool = fixture().await;

        let_assert!(
            SignupTagGrantAdded::Added(grant) = add_grant(
                &pool,
                "call",
                SignupTagBadge::Foundations,
                " /l/library-network-signup "
            )
            .await
            .unwrap()
        );
        check!(grant.next_path == "/l/library-network-signup");

        let listed = list_grants(&pool).await.unwrap();
        check!(listed[0].next_path == "/l/library-network-signup");
    }

    #[tokio::test]
    async fn a_path_off_this_site_is_refused() {
        let pool = fixture().await;

        for path in [
            "https://evil.example/",
            "//evil.example",
            "/\\evil.example",
            "l/x",
        ] {
            let added = add_grant(&pool, "call", SignupTagBadge::Foundations, path)
                .await
                .unwrap();
            check!(let SignupTagGrantAdded::NextPathNotLocal = added, "{path}");
        }

        let long = format!("/{}", "x".repeat(SIGNUP_TAG_GRANT_NEXT_PATH_LENGTH));
        let added = add_grant(&pool, "call", SignupTagBadge::Foundations, &long)
            .await
            .unwrap();
        check!(let SignupTagGrantAdded::NextPathTooLong = added);

        check!(tags(&pool).await.is_empty());
    }

    #[tokio::test]
    async fn the_mirror_carries_orrerys_path_and_drops_one_off_this_site() {
        let pool = fixture().await;
        let mut call = orrery(1, "call");
        call.next_path = "/l/library-network-signup".to_string();
        let mut azla = orrery(2, "azla");
        azla.next_path = "//evil.example".to_string();

        mirror(&pool, &[call, azla]).await.unwrap();

        let paths: Vec<(String, String)> = list_grants(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|g| (g.tag, g.next_path))
            .collect();
        check!(
            paths
                == vec![
                    ("azla".to_string(), String::new()),
                    ("call".to_string(), "/l/library-network-signup".to_string()),
                ]
        );
    }
}
