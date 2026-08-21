-- The vocabulary region-scoped search is expressed in, mirrored from
-- orrery's core_region. Hydra keeps its own copy of the polygons, and does
-- its own matching against them, so that it can answer a region search
-- without orrery being there to ask.
--
-- `digest` is the md5 orrery reports for the shape it sent, so a sync pass
-- can tell which polygons actually moved without pulling ten megabytes of
-- geometry it already has.
create table regions (
       "region_id" integer primary key,  -- orrery's core_region.id
       "name" text not null,
       "digest" text not null
);

create unique index regions_by_name on regions("name");

select AddGeometryColumn('regions', 'value', 4326, 'MULTIPOLYGON', 'XY');
select CreateSpatialIndex('regions', 'value');

-- Which regions each project falls in. A cache of what the matching rule
-- says, rebuilt from geometry and never edited directly: search joins this
-- rather than testing polygons per query.
create table project_regions (
       "project_id" text not null,
       "region_id" integer not null,
       primary key ("project_id", "region_id")
) strict;

create index project_regions_by_region on project_regions("region_id");
