-- People Finder requests: one message to everyone in an area, held for staff
-- review. Mirrors orrery's `messaging_peoplefinderapproval`.
--
-- A request keeps the question -- where, how far, and whom -- rather than a
-- list of recipients, because the recipients are worked out when staff
-- approve it, as orrery does. Approval names the `batch` its copies will
-- carry in `posts`, and `delivered_at` is stamped once the last copy exists,
-- so a delivery cut short by a restart is found and finished.
CREATE TABLE "people_finder_requests" (
    "request_id" text PRIMARY KEY NOT NULL,
    "sender_id" text NOT NULL,
    "created_at" text NOT NULL,
    "latitude" real NOT NULL,
    "longitude" real NOT NULL,
    "radius_miles" real NOT NULL,
    -- JSON array of `PeopleFinderFilter` names; a recipient is in all of them.
    "filters" text NOT NULL DEFAULT '[]',
    -- A `PeopleFinderReach` name.
    "reach" text NOT NULL,
    "subject" text NOT NULL,
    "body" text NOT NULL,
    -- 'pending', 'approved' or 'denied'.
    "status" text NOT NULL DEFAULT 'pending',
    "decided_at" text,
    "decided_by" text,
    "batch" text,
    "delivered_at" text
) STRICT;

CREATE INDEX "people_finder_requests_pending"
    ON "people_finder_requests" ("created_at")
    WHERE "status" = 'pending';

CREATE INDEX "people_finder_requests_undelivered"
    ON "people_finder_requests" ("decided_at")
    WHERE "status" = 'approved' AND "delivered_at" IS NULL;
