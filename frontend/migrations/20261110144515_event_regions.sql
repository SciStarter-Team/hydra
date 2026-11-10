-- Which regions each event falls in: project_regions' counterpart for
-- events, so an exchange can list the events in its region by the same
-- cached matching rule it lists projects by. Derived, never edited directly;
-- see frontend/src/region.rs. It starts empty and is filled by the region
-- sync's first pass after this deploys, since the rule it records now
-- covers events.
create table event_regions (
       "event_id" text not null,
       "region_id" integer not null,
       primary key ("event_id", "region_id")
) strict;

create index event_regions_by_region on event_regions("region_id");
