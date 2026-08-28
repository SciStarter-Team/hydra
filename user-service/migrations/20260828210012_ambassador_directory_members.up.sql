-- Who the public directory shows.
--
-- `users.ambassador` dates the promotion, but the verdict on the application
-- outranks it. Staff demote an Ambassador to Champion for a hiatus, and orrery
-- deliberately keeps the date so that the role can later be restored with the
-- one originally earned -- so the date alone leaves a demoted person listed,
-- and fifteen of them are listed today. hydra's own demotion clears the date,
-- but the user sync copies orrery's back in, so the rule cannot live in the
-- write path alone.
--
-- A view rather than a condition repeated in each query: the directory, the
-- profile, the neighbours strip, the map and the contact form all have to agree
-- about who is an Ambassador, and a rule written five times is a rule that
-- drifts. `5` is `AmbassadorAppStatus::Champion`, held to that number by
-- `champion_is_the_verdict_the_directory_view_excludes`.
--
-- Somebody with no application row at all keeps their place: several dozen
-- Ambassadors predate the application form, and no verdict exists to demote
-- them.
CREATE VIEW "ambassador_directory_members" AS
    SELECT u."user_id" AS "user_id"
      FROM "users" u
      LEFT JOIN "ambassador_applications" a ON a."user_id" = u."user_id"
     WHERE u."ambassador" IS NOT NULL
       AND COALESCE(a."app_status", 0) <> 5;
