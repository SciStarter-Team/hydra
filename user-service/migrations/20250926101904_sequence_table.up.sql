-- Create sequences table for managing distributed sequence generation
CREATE TABLE sequences (
    name TEXT PRIMARY KEY,
    current_value INTEGER NOT NULL
);

-- Initialize project_id sequence starting at 200000001
INSERT INTO sequences (name, current_value) VALUES ('project_id', 200000000);