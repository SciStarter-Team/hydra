-- Create table for storing final user set evaluation results
-- This allows fast paginated queries without re-computing set operations
CREATE TABLE user_set_results (
    evaluation_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    created_at TEXT NOT NULL
);

-- Indexes for efficient pagination and cleanup
CREATE INDEX idx_user_set_results_eval_id ON user_set_results(evaluation_id);
CREATE INDEX idx_user_set_results_created ON user_set_results(created_at);
