CREATE VIEW IF NOT EXISTS "user_tags_emails_cache" AS
  SELECT
    t."user_id" AS "user_id",
    t."tag" AS "tag",
    e."email" AS "email"
  FROM
    "user_tags_cache" t JOIN "user_emails_cache" e USING("user_id");
