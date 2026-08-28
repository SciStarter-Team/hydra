-- The directory of libraries in the network, as distinct from the sign-up
-- forms that people fill in.
--
-- Ported from orrery's `frontend_library`. It is maintained in a Google Sheet
-- that staff edit and imported here through `/staff/google-authorize`; the
-- import only ever adds libraries it has not seen before, so a hand-made
-- correction to a row is not overwritten on the next pass.
--
-- The name is the key, as it is in orrery: the sheet has no id column, and the
-- name is what the import deduplicates on.
--
-- A library with no `point` failed to geocode. The row is kept anyway, because
-- the directory is a list of libraries first and a map second, and the import
-- reports how many missed so staff can fix the addresses in the sheet.
CREATE TABLE "libraries" (
    "name" text PRIMARY KEY NOT NULL,
    "recorded" text NOT NULL,  -- TimeString
    "address" text NOT NULL DEFAULT '',
    "city" text NOT NULL DEFAULT '',
    "state" text NOT NULL DEFAULT '',
    "zipcode" text NOT NULL DEFAULT '',
    "org_type" text NOT NULL DEFAULT '',
    "url" text NOT NULL DEFAULT ''
);

SELECT AddGeometryColumn('libraries', 'point', 4326, 'POINT', 'XY');
SELECT CreateSpatialIndex('libraries', 'point');
