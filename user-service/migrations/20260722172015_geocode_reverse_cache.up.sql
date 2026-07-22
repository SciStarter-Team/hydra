-- Cache for reverse geocoding (point -> address components). Coordinates are
-- rounded to 5 decimal places (~1 m) before lookup/insert so nearby requests
-- share an entry.
CREATE TABLE geocode_reverse_cache (
    id TEXT PRIMARY KEY,
    latitude REAL NOT NULL,
    longitude REAL NOT NULL,
    formatted TEXT NOT NULL,
    road TEXT NOT NULL,
    house_number TEXT NOT NULL,
    city TEXT NOT NULL,
    state TEXT NOT NULL,
    postcode TEXT NOT NULL,
    country TEXT NOT NULL,
    country_code TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (latitude, longitude)
);
