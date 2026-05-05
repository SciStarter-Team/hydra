CREATE TABLE "user_places" (
    "id" text PRIMARY KEY NOT NULL,
    "user_id" text NOT NULL REFERENCES "users" ("user_id") ON DELETE CASCADE,
    "name" text NOT NULL,
    "created" text NOT NULL DEFAULT (current_timestamp)
);

SELECT AddGeometryColumn('user_places', 'point', 4326, 'POINT', 'XY');
SELECT CreateSpatialIndex('user_places', 'point');

CREATE INDEX "user_places_by_user_id" ON "user_places" ("user_id");
