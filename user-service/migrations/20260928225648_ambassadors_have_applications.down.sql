DROP VIEW "ambassador_directory_listing";
DROP VIEW "ambassador_directory_members";

CREATE VIEW "ambassador_directory_members" AS
    SELECT u."user_id" AS "user_id"
      FROM "users" u
      LEFT JOIN "ambassador_applications" a ON a."user_id" = u."user_id"
     WHERE u."ambassador" IS NOT NULL
       AND COALESCE(a."app_status", 0) <> 5;

CREATE VIEW "ambassador_directory_listing" AS
    SELECT m."user_id" AS "user_id",
           COALESCE(a."city", '') AS "city",
           COALESCE(a."state", '') AS "state",
           COALESCE(a."bio", '') AS "bio",
           COALESCE(a."youtube_video_url", '') AS "youtube_video_url",
           a."position" AS "position"
      FROM "ambassador_directory_members" m
      LEFT JOIN "ambassador_applications" a ON a."user_id" = m."user_id"
     WHERE COALESCE(a."hidden", 0) = 0;
