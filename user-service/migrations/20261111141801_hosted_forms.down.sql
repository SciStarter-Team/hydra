SELECT DisableSpatialIndex('form_submissions', 'location');
DROP TABLE IF EXISTS "idx_form_submissions_location";
SELECT DiscardGeometryColumn('form_submissions', 'location');
SELECT DiscardGeometryColumn('form_submissions', 'ip_location');
DROP TABLE "form_submissions";
DROP TABLE "form_owners";
DROP TABLE "forms";
