-- Add template data to sendings table so we can add targets to sendings even after queue is processed
ALTER TABLE sendings ADD COLUMN template_html TEXT NOT NULL DEFAULT '';
ALTER TABLE sendings ADD COLUMN data_sets TEXT NOT NULL DEFAULT '[]';
ALTER TABLE sendings ADD COLUMN additional_data TEXT NOT NULL DEFAULT '[]';
