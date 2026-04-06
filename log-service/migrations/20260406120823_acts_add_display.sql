ALTER TABLE acts_of_science ADD COLUMN display INTEGER NOT NULL DEFAULT 1;
CREATE INDEX idx_acts_display ON acts_of_science(display);
