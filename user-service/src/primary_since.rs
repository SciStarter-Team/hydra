//! When hydra became the primary server for scistarter.org.
//!
//! Records hydra makes itself mean what hydra says they mean only from this
//! instant: before it, the same kind of record was orrery's, made by orrery's
//! rules and copied here by sync, and hydra's own were only testing. So a
//! figure that should count only what hydra recorded counts from here, and
//! is not shown at all before there is such an instant.
//!
//! Nobody sets it. The first time a primary build of this service starts, it
//! writes down the time, and never again; a build that is not the primary
//! never does.

use anyhow::Error;
use common::timestring::TimeString;
use sqlx::SqlitePool;

/// The `misc` key the instant is kept under.
const KEY: &str = "primary-since";

/// Write down that hydra is the primary server as of now, unless it already
/// was.
pub async fn note(pool: &SqlitePool) -> Result<(), Error> {
    if !common::primary::PRIMARY {
        return Ok(());
    }

    let now = TimeString::now();

    sqlx::query!(
        r#"INSERT INTO "misc" ("key", "value") VALUES (?, ?)
           ON CONFLICT ("key") DO NOTHING"#,
        KEY,
        now,
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// When hydra became the primary server, if it has.
pub async fn read(pool: &SqlitePool) -> Result<Option<TimeString>, Error> {
    let since: Option<String> = sqlx::query_scalar(r#"SELECT "value" FROM "misc" WHERE "key" = ?"#)
        .bind(KEY)
        .fetch_optional(pool)
        .await?;

    Ok(since.map(TimeString::try_from).transpose()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    #[tokio::test]
    async fn read_is_none_until_noted() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");

        sqlx::raw_sql(
            r#"create table "misc" ("key" text primary key not null, "value" any not null) strict;"#,
        )
        .execute(&pool)
        .await
        .expect("schema");

        check!(read(&pool).await.unwrap() == None);

        sqlx::query(r#"insert into "misc" ("key", "value") values (?, ?)"#)
            .bind(KEY)
            .bind("2027-01-01T00:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        check!(
            read(&pool).await.unwrap()
                == Some(TimeString::try_from("2027-01-01T00:00:00Z".to_string()).unwrap())
        );
    }
}
