SELECT DisableSpatialIndex('libraries', 'point');
DROP TABLE IF EXISTS idx_libraries_point;
DROP TABLE IF EXISTS idx_libraries_point_node;
DROP TABLE IF EXISTS idx_libraries_point_parent;
DROP TABLE IF EXISTS idx_libraries_point_rowid;
SELECT DiscardGeometryColumn('libraries', 'point');
DROP TABLE IF EXISTS "libraries";
