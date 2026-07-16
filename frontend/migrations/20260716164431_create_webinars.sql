-- Webinars are SciStarter productions, or partner productions made with direct
-- SciStarter involvement. They are always authored by staff, which is what
-- separates them from events: events are user-submitted and synced from orrery,
-- so the two have different provenance, lifecycle and edit paths and are
-- deliberately not unified.
--
-- A webinar spans both the upcoming and archived cases seen on
-- pages.scistarter.org/library-webinars and /nasa-live. Which one a given
-- webinar is gets derived from "held_at" rather than stored, and the
-- presentation difference (Register vs Watch Recording) falls out of which
-- links it carries.
CREATE TABLE "webinars" (
    "webinar_id"           TEXT PRIMARY KEY NOT NULL, -- Ulid
    "created"              TEXT NOT NULL DEFAULT '',  -- TimeString
    "updated"              TEXT NOT NULL DEFAULT '',  -- TimeString
    "publish"              BOOLEAN NOT NULL DEFAULT 0,
    "title"                TEXT NOT NULL DEFAULT '',
    "description"          TEXT NOT NULL DEFAULT '',
    "held_at"              TEXT NOT NULL DEFAULT '',  -- TimeString
    "platform"             TEXT NOT NULL DEFAULT '',
    "image"                TEXT NOT NULL DEFAULT '',
    "image_credit"         TEXT NOT NULL DEFAULT '',
    -- Slug of the project or event this webinar features, as shown by the
    -- "Featured Project:" line on /nasa-live. Resolved through "entities".
    "featured_entity_slug" TEXT NOT NULL DEFAULT ''
);

CREATE INDEX "webinars_by_held_at" ON "webinars" ("held_at");

-- The audience-facing filter vocabulary, kept deliberately separate from
-- "event_tags": webinar tags are page-filter terms ("For the Public",
-- "Citizen Science Kits"), and routing them through event_tags would leak them
-- into entities_fts and the site-wide tag autocomplete.
CREATE TABLE "webinar_tags" (
    "webinar_id" TEXT NOT NULL REFERENCES "webinars"("webinar_id") ON DELETE CASCADE,
    "tag"        TEXT NOT NULL
);

CREATE INDEX "webinar_tags_by_tag" ON "webinar_tags" (upper("tag"));
CREATE UNIQUE INDEX "webinar_tags_by_webinar_and_tag" ON "webinar_tags" ("webinar_id", upper("tag"));

-- Typed resource links. Several of a kind may coexist: a webinar offering
-- "Choose a session: 6 PM ET / 9 PM ET" is one webinar carrying two Register
-- links whose labels are the session times. Rewritten wholesale with the parent
-- webinar, so rows need no identity of their own.
CREATE TABLE "webinar_links" (
    "webinar_id" TEXT NOT NULL REFERENCES "webinars"("webinar_id") ON DELETE CASCADE,
    "kind"       TEXT NOT NULL DEFAULT '', -- WebinarLinkKind
    "label"      TEXT NOT NULL DEFAULT '',
    "url"        TEXT NOT NULL DEFAULT '',
    "priority"   INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX "webinar_links_by_webinar" ON "webinar_links" ("webinar_id");
