-- An Ambassador has an application.
--
-- Membership used to keep anybody with a `users.ambassador` date and no
-- application row, on the belief that several dozen Ambassadors predated the
-- application form. They did not: their applications had been deleted here
-- by the user sync's INSERT OR REPLACE cascading through `users`, and once
-- they were restored every Ambassador had one. orrery never promotes anybody
-- without an application (none reach `passed` without one), so the only
-- account the old rule still admitted was a staff account carrying a stray
-- date, which it listed publicly and credited with the role.
--
-- Membership now requires the application, as orrery's does. The listing is
-- rebuilt on it, and reads the application's fields directly now that one is
-- always there.
DROP VIEW "ambassador_directory_listing";
DROP VIEW "ambassador_directory_members";

-- `5` is `AmbassadorAppStatus::Champion`, held to that number by
-- `champion_is_the_verdict_the_directory_view_excludes`.
CREATE VIEW "ambassador_directory_members" AS
    SELECT u."user_id" AS "user_id"
      FROM "users" u
      JOIN "ambassador_applications" a ON a."user_id" = u."user_id"
     WHERE u."ambassador" IS NOT NULL
       AND a."app_status" <> 5;

CREATE VIEW "ambassador_directory_listing" AS
    SELECT m."user_id" AS "user_id",
           a."city" AS "city",
           a."state" AS "state",
           a."bio" AS "bio",
           a."youtube_video_url" AS "youtube_video_url",
           a."position" AS "position"
      FROM "ambassador_directory_members" m
      JOIN "ambassador_applications" a ON a."user_id" = m."user_id"
     WHERE a."hidden" = 0;
