-- Library Network signups. Backs the public sign-up form, the staff browser
-- and the map export.
--
-- Rows arrive two ways: from our own form, and mirrored from orrery's
-- `frontend_librarynetworksignup` while orrery still serves that form. Hence
-- the two keys — `signup_id` is ours and always present, `orrery_id` is the
-- sync's conflict target and is null for anything created here. SQLite allows
-- repeated NULLs in a UNIQUE column, so every native row can leave it empty.
--
-- Deliberately no foreign key to `users`: the form does not require an
-- account, so `user_id` is often absent, and when it is present the user may
-- not have synced across yet — a constraint would reject the signup rather
-- than let it arrive early.
CREATE TABLE "library_network_signups" (
    "signup_id" text PRIMARY KEY NOT NULL,
    "orrery_id" integer UNIQUE,
    "recorded" text NOT NULL,
    "updated" text NOT NULL,
    "user_id" text,
    "email" text NOT NULL DEFAULT '',
    "first_name" text NOT NULL DEFAULT '',
    "last_name" text NOT NULL DEFAULT '',
    "job_title" text NOT NULL DEFAULT '',
    "have_account" integer NOT NULL DEFAULT 0,
    "organization_name" text NOT NULL DEFAULT '',
    "organization_type" text NOT NULL DEFAULT '',
    "primary_job_type" text NOT NULL DEFAULT '',
    "organization_street_address" text NOT NULL DEFAULT '',
    "organization_city" text NOT NULL DEFAULT '',
    "organization_state" text NOT NULL DEFAULT '',
    "organization_zip_code" text NOT NULL DEFAULT '',
    "organization_country" text NOT NULL DEFAULT '',
    "organization_web_page" text NOT NULL DEFAULT '',
    "organization_social_handles" text NOT NULL DEFAULT '',
    "add_to_map" integer NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX "library_network_signups_by_recorded" ON "library_network_signups" ("recorded");
CREATE INDEX "library_network_signups_by_add_to_map" ON "library_network_signups" ("add_to_map");
CREATE INDEX "library_network_signups_by_user_id" ON "library_network_signups" ("user_id");
