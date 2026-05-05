DROP INDEX IF EXISTS "user_places_by_user_id";
SELECT DiscardGeometryColumn('user_places', 'point');
DROP TABLE IF EXISTS "user_places";
