CREATE TABLE IF NOT EXISTS claim_codes (
    id INTEGER PRIMARY KEY NOT NULL,
    code TEXT NOT NULL,
    category TEXT NOT NULL,
    claimed INTEGER NOT NULL DEFAULT 0,
    claimed_by TEXT,
    claimed_at TEXT
);

CREATE INDEX idx_claim_codes_category_claimed ON claim_codes(category, claimed);
CREATE INDEX idx_claim_codes_category_claimed_by ON claim_codes(category, claimed_by);
