-- The legacy site's own annual engagement tallies, one row per calendar year.
--
-- `/data` reports a year's page views, saves, referrals and contributions.
-- For years the legacy site served, this system cannot recompute them: it
-- never recorded those page views at all, and the event streams it did
-- synchronize are a copy that is neither complete nor free of duplicates. So
-- the legacy tallies are authoritative for the years listed here, and
-- `log.stats.annual_engagement` computes a year from our own logs only when
-- no row for it exists.
--
-- These values are the legacy site's `frontend_annualengagement` rows. When
-- the legacy site is retired, add a row here for its final, partial year, so
-- that the year we served jointly is reported from the one tally that saw all
-- of it; every year after that is ours to compute.
CREATE TABLE legacy_engagement (
    "year"          INTEGER PRIMARY KEY NOT NULL,
    "views"         INTEGER NOT NULL,
    "saves"         INTEGER NOT NULL,
    "referrals"     INTEGER NOT NULL,
    "contributions" INTEGER NOT NULL
);

INSERT INTO legacy_engagement ("year", "views", "saves", "referrals", "contributions") VALUES
    (2019, 7165460, 37919, 35612, 7591548),
    (2020, 5783188,  6672, 34115, 3418453),
    (2021, 5050982,  7019, 35080, 2702007),
    (2022, 4821637,  6878, 34866, 2355521),
    (2023, 6043096,  6692, 29931, 2149679),
    (2024, 6646246, 26239, 91211, 2671631),
    (2025, 9745912, 86745, 274802, 3024009);
