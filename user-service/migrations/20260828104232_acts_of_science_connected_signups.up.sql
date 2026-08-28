-- Applications to host an Acts of Science Connected event at a library.
--
-- Ported from orrery's `frontend_libraryactsofscienceconnectedsignup`. The
-- form lives on a curated page and posts here; staff read the result as a CSV
-- and pick which libraries to run events with. Nothing edits a row after it
-- arrives, so there is no `updated` column — this is a record of what someone
-- submitted, not a profile.
--
-- The availability columns are the grid of dates and times the applicant can
-- host, one column per slot, exactly as orrery names them. They are free text
-- rather than booleans because the form lets people qualify an answer ("after
-- 3pm only"), and staff read that text when scheduling.
--
-- Deliberately no foreign key to `users`: applying does not require an
-- account, and the contact is identified by the address they gave.
CREATE TABLE "acts_of_science_connected_signups" (
    "signup_id" text PRIMARY KEY NOT NULL,  -- ULID
    "recorded" text NOT NULL,               -- TimeString

    "library_name" text NOT NULL DEFAULT '',
    "address1" text NOT NULL DEFAULT '',
    "address2" text NOT NULL DEFAULT '',
    "city" text NOT NULL DEFAULT '',
    "state" text NOT NULL DEFAULT '',
    "zip" text NOT NULL DEFAULT '',
    "country" text NOT NULL DEFAULT '',

    "contact_first" text NOT NULL DEFAULT '',
    "contact_last" text NOT NULL DEFAULT '',
    "contact_email" text NOT NULL DEFAULT '',
    "contact_phone" text NOT NULL DEFAULT '',

    "service_area" text NOT NULL DEFAULT '',
    "seating_capacity" text NOT NULL DEFAULT '',
    "tech_support" text NOT NULL DEFAULT '',

    "event1aft" text NOT NULL DEFAULT '',
    "event1eve" text NOT NULL DEFAULT '',
    "event2aft" text NOT NULL DEFAULT '',
    "event2eve" text NOT NULL DEFAULT '',
    "event3aft" text NOT NULL DEFAULT '',
    "event3mid" text NOT NULL DEFAULT '',
    "event3eve" text NOT NULL DEFAULT '',
    "event4aft" text NOT NULL DEFAULT '',
    "event4eve" text NOT NULL DEFAULT '',
    "event5aft" text NOT NULL DEFAULT '',
    "event5eve" text NOT NULL DEFAULT '',
    "eventpromote" text NOT NULL DEFAULT '',

    "additional_programming" text NOT NULL DEFAULT '',
    "why_interested" text NOT NULL DEFAULT '',
    "requirements" text NOT NULL DEFAULT '',
    "wired_ethernet" text NOT NULL DEFAULT '',
    "available_devices" text NOT NULL DEFAULT '',
    "comments" text NOT NULL DEFAULT '',
    "request_grant" text NOT NULL DEFAULT 'No'
) STRICT;

CREATE INDEX "acts_of_science_connected_signups_by_recorded"
    ON "acts_of_science_connected_signups" ("recorded");
