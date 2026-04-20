ALTER TABLE acts_of_science ADD COLUMN featured INTEGER NOT NULL DEFAULT 0;
ALTER TABLE acts_of_science ADD COLUMN sampled INTEGER NOT NULL DEFAULT 0;
CREATE INDEX idx_acts_featured ON acts_of_science(featured);
CREATE INDEX idx_acts_sampled ON acts_of_science(sampled);
