-- Editable text content for training pages, one row per training workflow.
-- The structural/gating logic for each training lives in code; this table holds
-- only the admin-editable copy, stored as a JSON blob of the training's typed
-- content struct. Read through the LoadStorePeered training content records and
-- edited via the Engine Room admin.
CREATE TABLE "training_content" (
    "workflow" TEXT PRIMARY KEY NOT NULL,
    "content"  TEXT NOT NULL
);
