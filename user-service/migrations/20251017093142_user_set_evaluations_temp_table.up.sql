-- Create table for storing temporary user set evaluation results
-- This allows us to process large user sets in chunks without loading everything into memory
CREATE TABLE user_set_evaluations (
    evaluation_id TEXT NOT NULL,
    criterion_index INTEGER NOT NULL,
    operation TEXT NOT NULL, -- 'union' or 'intersect'
    user_id TEXT NOT NULL,
    created_at TEXT NOT NULL
);

-- Indexes for efficient set operations
CREATE INDEX idx_user_set_evaluations_eval_id ON user_set_evaluations(evaluation_id);
CREATE INDEX idx_user_set_evaluations_eval_op ON user_set_evaluations(evaluation_id, operation);
CREATE INDEX idx_user_set_evaluations_eval_criterion ON user_set_evaluations(evaluation_id, criterion_index);
CREATE INDEX idx_user_set_evaluations_created ON user_set_evaluations(created_at);
