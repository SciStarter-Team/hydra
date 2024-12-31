use crate::shared::db;

pub async fn get<T>(key: impl AsRef<str>) -> Result<Option<T>, sqlx::Error>
where
    T: Send + Unpin + sqlx::Type<sqlx::Sqlite> + for<'a> sqlx::Decode<'a, sqlx::Sqlite>,
{
    let key = key.as_ref();
    Ok(
        sqlx::query_scalar(r#"select "value" as "value: T" from "misc" where "key" = ?"#)
            .bind(key)
            .fetch_optional(db())
            .await?,
    )
}

pub async fn set<T>(key: impl AsRef<str>, value: T) -> Result<(), sqlx::Error>
where
    T: sqlx::Type<sqlx::Sqlite> + for<'a> sqlx::Encode<'a, sqlx::Sqlite>,
{
    let key = key.as_ref();
    sqlx::query(r#"insert or replace into "misc" ("key", "value") values (?, ?)"#)
        .bind(key)
        .bind(value)
        .execute(db())
        .await?;
    Ok(())
}

pub async fn del(key: impl AsRef<str>) -> Result<(), sqlx::Error> {
    let key = key.as_ref();
    sqlx::query(r#"delete from "misc" where "key" = ?"#)
        .bind(key)
        .execute(db())
        .await?;
    Ok(())
}
