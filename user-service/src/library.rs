//! The Library Network: who signed up, and which libraries are in it.
//!
//! Two separate things live here, both belonging to the same programme.
//!
//! The **signups** are the people who filled in the sign-up form. Those rows
//! are mirrored from orrery (see `library_signups_synchronize` in main.rs),
//! which owns the form and stays the system of record; this service only reads
//! them: a paged, filtered list for the grid, a wide CSV export, and the narrow
//! deduplicated "Prepare Map" export.
//!
//! The **directory** is the list of libraries themselves, maintained by staff
//! in a Google Sheet and imported through `/staff/google-authorize`. Unlike the
//! signups, this service owns it.

use std::collections::{BTreeSet, HashSet};

use anyhow::Error;
use async_nats::service::Request;
use common::{
    message::dashboard::{
        LibraryDirectoryEntry, LibraryDirectoryRecordResponse, LibraryMapRow, LibrarySignupFilters,
        LibrarySignupListRequest, LibrarySignupListResponse, LibrarySignupRow, LibrarySignupUpdate,
    },
    organizations,
    service::decode_structured_message,
    shared::{db, hash_identifier_lower},
    timestring::TimeString,
};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use ulid::Ulid;

/// Kept in step with `LibraryNetworkSignup.HAVE_ACCOUNT` in orrery's
/// frontend/models.py.
fn have_account_label(value: i64) -> &'static str {
    match value {
        0 => "No",
        1 => "Yes",
        _ => "I don't know",
    }
}

/// Push the browser's filters onto a query already ending in a WHERE clause.
/// Shared by the list, its count, and the CSV export so the three cannot
/// disagree about what "matching" means.
fn push_filters(qb: &mut QueryBuilder<Sqlite>, f: &LibrarySignupFilters) {
    fn contains(qb: &mut QueryBuilder<Sqlite>, column: &str, needle: &str) {
        let trimmed = needle.trim();
        if trimmed.is_empty() {
            return;
        }
        qb.push(" AND \"");
        qb.push(column);
        qb.push("\" LIKE ");
        qb.push_bind(format!("%{}%", trimmed));
    }

    if let Some(b) = f.add_to_map.as_bool() {
        qb.push(" AND \"add_to_map\" = ");
        qb.push_bind(if b { 1_i64 } else { 0_i64 });
    }

    if !f.have_account.is_empty() {
        qb.push(" AND \"have_account\" IN (");
        let mut first = true;
        for v in &f.have_account {
            if !first {
                qb.push(", ");
            }
            first = false;
            qb.push_bind(*v);
        }
        qb.push(")");
    }

    let from = f.recorded_from.trim();
    if !from.is_empty() {
        qb.push(" AND \"recorded\" >= ");
        qb.push_bind(from.to_string());
    }

    let to = f.recorded_to.trim();
    if !to.is_empty() {
        // `recorded` carries a time, so an inclusive end date has to cover the
        // whole of that day.
        qb.push(" AND \"recorded\" <= ");
        qb.push_bind(format!("{to}T23:59:59.999999Z"));
    }

    contains(qb, "organization_name", &f.organization_name);
    contains(qb, "organization_type", &f.organization_type);
    contains(qb, "primary_job_type", &f.primary_job_type);
    contains(qb, "organization_city", &f.organization_city);
    contains(qb, "organization_state", &f.organization_state);
    contains(qb, "organization_zip_code", &f.organization_zip_code);
    contains(qb, "organization_country", &f.organization_country);
    contains(qb, "organization_web_page", &f.organization_web_page);
    contains(qb, "email", &f.email);
    contains(qb, "job_title", &f.job_title);

    let search = f.search.trim();
    if !search.is_empty() {
        let pattern = format!("%{}%", search);
        qb.push(" AND (\"organization_name\" LIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" OR \"email\" LIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" OR \"first_name\" LIKE ");
        qb.push_bind(pattern.clone());
        qb.push(" OR \"last_name\" LIKE ");
        qb.push_bind(pattern);
        qb.push(")");
    }
}

const ROW_COLUMNS: &str = r#"
    "signup_id", "orrery_id", "recorded", "user_id", "email", "first_name", "last_name",
    "job_title", "have_account", "organization_name", "organization_type",
    "primary_job_type", "organization_street_address", "organization_city",
    "organization_state", "organization_zip_code", "organization_country",
    "organization_web_page", "organization_social_handles", "add_to_map"
"#;

const FROM_WHERE: &str = r#" FROM "library_network_signups" WHERE 1 = 1"#;

fn signup_row(row: &sqlx::sqlite::SqliteRow) -> LibrarySignupRow {
    LibrarySignupRow {
        signup_id: row.get("signup_id"),
        orrery_id: row.get("orrery_id"),
        recorded: row.get("recorded"),
        user_ulid: row.get::<Option<String>, _>("user_id").unwrap_or_default(),
        email: row.get("email"),
        first_name: row.get("first_name"),
        last_name: row.get("last_name"),
        job_title: row.get("job_title"),
        have_account: have_account_label(row.get("have_account")).to_string(),
        organization_name: row.get("organization_name"),
        organization_type: row.get("organization_type"),
        primary_job_type: row.get("primary_job_type"),
        organization_street_address: row.get("organization_street_address"),
        organization_city: row.get("organization_city"),
        organization_state: row.get("organization_state"),
        organization_zip_code: row.get("organization_zip_code"),
        organization_country: row.get("organization_country"),
        organization_web_page: row.get("organization_web_page"),
        organization_social_handles: row.get("organization_social_handles"),
        add_to_map: row.get::<i64, _>("add_to_map") != 0,
    }
}

/// One page of signups for the staff grid (`user.library.list`), plus the
/// total number of matches so the page can render its pagination.
pub async fn list(req: &Request) -> Result<LibrarySignupListResponse, Error> {
    let request: LibrarySignupListRequest =
        decode_structured_message(req.message.clone()).unwrap_or_default();

    let mut count_qb: QueryBuilder<Sqlite> =
        QueryBuilder::new(format!("SELECT COUNT(*){FROM_WHERE}"));
    push_filters(&mut count_qb, &request.filters);
    let total: i64 = count_qb.build_query_scalar().fetch_one(db()).await?;

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new("SELECT ");
    qb.push(ROW_COLUMNS);
    qb.push(FROM_WHERE);
    push_filters(&mut qb, &request.filters);
    qb.push(" ORDER BY \"recorded\" DESC, \"signup_id\" DESC");

    if request.per_page > 0 {
        let page = request.page.max(1);
        qb.push(" LIMIT ");
        qb.push_bind(request.per_page);
        qb.push(" OFFSET ");
        qb.push_bind((page - 1) * request.per_page);
    }

    let rows = qb.build().fetch_all(db()).await?;

    Ok(LibrarySignupListResponse {
        rows: rows.iter().map(signup_row).collect(),
        total,
    })
}

/// The columns a mapping tool wants (`user.library.map`), one row per distinct
/// organization.
///
/// Signups are append-only, so two librarians from the same building each
/// opting in would otherwise stack two pins on the same spot. Rows are
/// collapsed to organizations by `common::organizations` — the rule orrery's
/// map export and both Moore ledgers use, which joins rows agreeing on a
/// normalized name or a normalized address — and the surviving row is the most
/// recent: the freshest description of a library whose web page or contact may
/// have changed. Values are trimmed on the way out, since stray padding is
/// noise to whatever geocodes the file.
///
/// Deliberately unfiltered: it answers one fixed question — which libraries
/// asked to appear — so a filter left over from browsing cannot silently
/// shrink the map.
pub async fn map(_req: &Request) -> Result<Vec<LibraryMapRow>, Error> {
    // Newest first, so that the first row of each organization is its
    // freshest description.
    let rows = sqlx::query!(
        r#"SELECT
             trim("organization_name") AS "organization_name!: String",
             trim("organization_street_address") AS "organization_street_address!: String",
             trim("organization_city") AS "organization_city!: String",
             trim("organization_state") AS "organization_state!: String",
             trim("organization_zip_code") AS "organization_zip_code!: String",
             trim("organization_country") AS "organization_country!: String",
             trim("organization_web_page") AS "organization_web_page!: String"
           FROM "library_network_signups"
           WHERE "add_to_map" = 1
           ORDER BY "recorded" DESC, "orrery_id" DESC, "signup_id" DESC"#
    )
    .fetch_all(db())
    .await?;

    let keep: HashSet<usize> = organizations::first_of_each(
        &rows
            .iter()
            .map(|row| organizations::Identity {
                name: &row.organization_name,
                address: &row.organization_street_address,
                zip: &row.organization_zip_code,
            })
            .collect::<Vec<_>>(),
    )
    .into_iter()
    .collect();

    let mut map: Vec<LibraryMapRow> = rows
        .into_iter()
        .enumerate()
        .filter(|(index, _)| keep.contains(index))
        .map(|(_, row)| LibraryMapRow {
            organization_name: row.organization_name,
            organization_street_address: row.organization_street_address,
            organization_city: row.organization_city,
            organization_state: row.organization_state,
            organization_zip_code: row.organization_zip_code,
            organization_country: row.organization_country,
            organization_web_page: row.organization_web_page,
        })
        .collect();

    map.sort_by(|a, b| {
        a.organization_name
            .to_lowercase()
            .cmp(&b.organization_name.to_lowercase())
            .then_with(|| {
                a.organization_city
                    .to_lowercase()
                    .cmp(&b.organization_city.to_lowercase())
            })
    });

    Ok(map)
}

/// One signup, for the staff edit form (`user.library.get`).
pub async fn get(req: &Request) -> Result<Option<LibrarySignupRow>, Error> {
    let signup_id: String = decode_structured_message(req.message.clone())?;

    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new("SELECT ");
    qb.push(ROW_COLUMNS);
    qb.push(FROM_WHERE);
    qb.push(" AND \"signup_id\" = ");
    qb.push_bind(signup_id);

    Ok(qb
        .build()
        .fetch_optional(db())
        .await?
        .map(|r| signup_row(&r)))
}

/// Apply a staff edit (`user.library.save`).
///
/// Identity and provenance columns are not editable: `signup_id` names the
/// row, `orrery_id` records where it came from, and `recorded` is when the
/// librarian filled the form in. `updated` moves so the row sorts as recently
/// touched, matching what orrery's `auto_now` does.
pub async fn save(req: &Request) -> Result<(), Error> {
    let update: LibrarySignupUpdate = decode_structured_message(req.message.clone())?;
    let now = TimeString::now().to_string();
    let add_to_map = if update.add_to_map { 1_i64 } else { 0_i64 };

    sqlx::query!(
        r#"UPDATE "library_network_signups" SET
             "updated" = ?,
             "email" = ?,
             "first_name" = ?,
             "last_name" = ?,
             "job_title" = ?,
             "have_account" = ?,
             "organization_name" = ?,
             "organization_type" = ?,
             "primary_job_type" = ?,
             "organization_street_address" = ?,
             "organization_city" = ?,
             "organization_state" = ?,
             "organization_zip_code" = ?,
             "organization_country" = ?,
             "organization_web_page" = ?,
             "organization_social_handles" = ?,
             "add_to_map" = ?
           WHERE "signup_id" = ?"#,
        now,
        update.email,
        update.first_name,
        update.last_name,
        update.job_title,
        update.have_account,
        update.organization_name,
        update.organization_type,
        update.primary_job_type,
        update.organization_street_address,
        update.organization_city,
        update.organization_state,
        update.organization_zip_code,
        update.organization_country,
        update.organization_web_page,
        update.organization_social_handles,
        add_to_map,
        update.signup_id,
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Record a signup from our own form (`user.library.create`).
///
/// `orrery_id` stays null: this row did not come from there, and leaving it
/// null is what keeps the sync's upsert from ever matching it.
pub async fn create(req: &Request) -> Result<(), Error> {
    let signup: LibrarySignupUpdate = decode_structured_message(req.message.clone())?;
    let signup_id = Ulid::new().to_string();
    let now = TimeString::now().to_string();
    let add_to_map = if signup.add_to_map { 1_i64 } else { 0_i64 };

    sqlx::query!(
        r#"INSERT INTO "library_network_signups" (
             "signup_id", "recorded", "updated", "user_id", "email", "first_name",
             "last_name", "job_title", "have_account", "organization_name",
             "organization_type", "primary_job_type", "organization_street_address",
             "organization_city", "organization_state", "organization_zip_code",
             "organization_country", "organization_web_page",
             "organization_social_handles", "add_to_map"
           ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        signup_id,
        now,
        now,
        signup.user_ulid,
        signup.email,
        signup.first_name,
        signup.last_name,
        signup.job_title,
        signup.have_account,
        signup.organization_name,
        signup.organization_type,
        signup.primary_job_type,
        signup.organization_street_address,
        signup.organization_city,
        signup.organization_state,
        signup.organization_zip_code,
        signup.organization_country,
        signup.organization_web_page,
        signup.organization_social_handles,
        add_to_map,
    )
    .execute(db())
    .await?;

    Ok(())
}

/// Every account in the Library Network: those a sign-up names, by its
/// account link or, for a sign-up made signed out, by the address it gives.
///
/// The address is matched case-insensitively against each account's primary
/// address only, as orrery's `LibraryNetworkSignup.accounts_among` matches
/// `User.QUERY_EMAIL`, so the two sites agree on who is in the network. The
/// match goes through `emails.hash_lower`, which is indexed; `users` has no
/// case-insensitive index on its address, and scanning it is not cheap.
pub async fn network_accounts(pool: &SqlitePool) -> Result<BTreeSet<String>, Error> {
    let signups =
        sqlx::query!(r#"SELECT "user_id", "email" AS "email!" FROM "library_network_signups""#)
            .fetch_all(pool)
            .await?;

    let mut accounts = BTreeSet::new();
    let mut hashes = Vec::new();

    for signup in signups {
        if let Some(user_id) = signup.user_id {
            accounts.insert(user_id);
        }

        let email = signup.email.trim();
        if !email.is_empty() {
            hashes.push(hash_identifier_lower(email));
        }
    }

    let hashes = serde_json::to_string(&hashes)?;
    let matched = sqlx::query_scalar!(
        r#"SELECT e."user_id" AS "user_id!"
             FROM "emails" e
             JOIN "users" u ON u."user_id" = e."user_id"
            WHERE e."hash_lower" IN (SELECT "value" FROM json_each(?))
              AND lower(e."email") = lower(u."primary_email")"#,
        hashes
    )
    .fetch_all(pool)
    .await?;

    accounts.extend(matched);

    Ok(accounts)
}

/// Add one library to the directory (`user.library.directory_record`), unless
/// a library of that name is already there.
///
/// Insert-only by design, matching orrery's import: the sheet is the place to
/// correct a library's details, but a row already imported may since have been
/// fixed by hand here, and a re-import must not undo that. `INSERT OR IGNORE`
/// makes "already known" the normal outcome rather than an error, so the
/// import can run repeatedly and only pick up what is new.
pub async fn directory_record(req: &Request) -> Result<LibraryDirectoryRecordResponse, Error> {
    let entry: LibraryDirectoryEntry =
        decode_structured_message(req.message.clone()).unwrap_or_default();

    if entry.name.trim().is_empty() {
        return Ok(LibraryDirectoryRecordResponse { added: false });
    }

    let recorded = TimeString::now();

    // An empty point stores as NULL rather than as an unparseable geometry, so
    // "no location" is one thing in the column rather than two.
    let point = (!entry.point.trim().is_empty()).then_some(entry.point.as_str());

    let result = sqlx::query!(
        r#"INSERT OR IGNORE INTO "libraries"
             ("name", "recorded", "address", "city", "state", "zipcode", "org_type", "url", "point")
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, SetSRID(GeomFromGeoJSON(?), 4326))"#,
        entry.name,
        recorded,
        entry.address,
        entry.city,
        entry.state,
        entry.zipcode,
        entry.org_type,
        entry.url,
        point,
    )
    .execute(db())
    .await?;

    Ok(LibraryDirectoryRecordResponse {
        added: result.rows_affected() > 0,
    })
}

/// The whole directory (`user.library.directory_list`), by name.
///
/// Unpaginated: the directory is a few thousand libraries at most, and both
/// callers — the import, which needs to know what it already has, and the
/// staff listing — want all of it.
pub async fn directory_list(_req: &Request) -> Result<Vec<LibraryDirectoryEntry>, Error> {
    let rows = sqlx::query!(
        r#"SELECT "name" AS "name!", "recorded" AS "recorded!", "address" AS "address!",
                  "city" AS "city!", "state" AS "state!", "zipcode" AS "zipcode!",
                  "org_type" AS "org_type!", "url" AS "url!",
                  AsGeoJSON("point") AS "point?: String"
             FROM "libraries"
            ORDER BY "name""#
    )
    .fetch_all(db())
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| LibraryDirectoryEntry {
            name: row.name,
            address: row.address,
            city: row.city,
            state: row.state,
            zipcode: row.zipcode,
            org_type: row.org_type,
            url: row.url,
            point: row.point.unwrap_or_default(),
            recorded: row.recorded,
        })
        .collect())
}
