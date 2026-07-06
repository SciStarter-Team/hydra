-- Editable, singleton site-settings blobs, one row per settings key.
-- The structural logic lives in code; this table holds only the
-- admin-editable configuration, stored as a JSON blob of the typed settings
-- struct. Read through / written by LoadStorePeered singleton records (e.g.
-- HomeSettings) and edited via the Home Page / Dashboard admin.
CREATE TABLE "site_settings" (
    "key"     TEXT PRIMARY KEY NOT NULL,
    "content" TEXT NOT NULL
);
