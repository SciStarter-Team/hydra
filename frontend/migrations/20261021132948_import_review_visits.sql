-- When each staff member last looked at /staff/import-review
-- (crate::staff::import_review::ImportReviewVisit, peered between frontend
-- instances). Without a date of its own, the page shows the projects that
-- arrived since the viewer's previous visit. viewed_at is a TimeString.
-- Mirrors orrery's staff_importreviewvisit.

CREATE TABLE import_review_visits (
    user_id TEXT NOT NULL PRIMARY KEY,
    viewed_at TEXT NOT NULL
) STRICT;
