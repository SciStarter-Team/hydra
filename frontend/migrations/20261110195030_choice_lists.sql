-- The lists that projects' and events' multiple-choice fields pick from --
-- event types, topics, activities and audience -- are data that staff keep at
-- /-admin/choice-lists, held in one table, and what each project and event
-- picks from them in one other. See frontend/src/choice.rs.
--
-- A value is keyed within its list by a slug that never changes once it is
-- made, so renaming a value leaves what picks it alone. Until cutover the
-- lists are copied from orrery, which keys them by its own ids: orrery_id
-- holds that id, and is how the copy, the syncs and the orrery-compatible APIs
-- find a value.
create table choice_values (
       "list" text not null,
       "slug" text not null,
       "name" text not null collate nocase,
       "position" integer not null,

       -- Which question an event type answers: 'format', 'program', 'venue',
       -- or '' for a value of a list that asks only one.
       "facet" text not null default '',

       "orrery_id" integer,

       primary key ("list", "slug"),
       unique ("list", "name"),
       unique ("list", "orrery_id")
) strict;

-- No foreign key to choice_values: a peer can receive a project or event
-- before a value it picks, and the join through choice_names below skips an
-- assignment whose value is not (or not yet) there.
create table choice_assignments (
       "target" text not null,
       "list" text not null,
       "value" text not null,
       primary key ("target", "list", "value")
) strict;

create index choice_assignments_by_value on choice_assignments ("list", "value");

-- What each project and event picks, with the names, positions and facets of
-- the values: the one place the two tables are joined, for everything that
-- shows or searches by them.
create view choice_names (target, list, value, name, position, facet) as
select a.target, a.list, a.value, v.name, v.position, v.facet
from choice_assignments a
join choice_values v on v.list = a.list and v.slug = a.value;

-- Orrery's lists as they stand, with its ids and order. The event types'
-- facets are the ones orrery is giving them; kid-friendly, which it is
-- retiring in favour of the Audience list, has none, and the copy from orrery
-- removes it once orrery has.
insert into choice_values ("list", "slug", "name", "position", "facet", "orrery_id") values
       ('EventType', 'acts-of-science-connected', 'Acts of Science Connected', 0, 'program', 459),
       ('EventType', 'nasa-night-sky-network', 'NASA Night Sky Network', 1, 'program', 460),
       ('EventType', 'ambassador', 'Ambassador', 2, 'program', 424),
       ('EventType', 'bioblitz', 'BioBlitz', 3, 'format', 187),
       ('EventType', 'citizen-forum', 'Citizen forum', 4, 'format', 185),
       ('EventType', 'city-nature-challenge', 'City Nature Challenge', 5, 'program', 186),
       ('EventType', 'conference', 'Conference', 6, 'format', 395),
       ('EventType', 'do-nasa-science-live', 'Do NASA Science Live', 7, 'program', 458),
       ('EventType', 'hackfest', 'hackfest', 8, 'format', 188),
       ('EventType', 'international-monarch-monitoring-blitz', 'International Monarch Monitoring Blitz', 9, 'program', 212),
       ('EventType', 'kid-friendly', 'kid-friendly', 10, '', 189),
       ('EventType', 'library', 'library', 11, 'venue', 406),
       ('EventType', 'megathon', 'Megathon', 12, 'format', 204),
       ('EventType', 'museum-event', 'museum event', 13, 'venue', 190),
       ('EventType', 'nature-monitoring', 'Nature monitoring', 14, 'format', 404),
       ('EventType', 'online', 'Online', 15, 'format', 405),
       ('EventType', 'professional-development', 'professional development', 16, 'format', 191),
       ('EventType', 'science-festival', 'science festival', 17, 'format', 192),
       ('EventType', 'social-media-event', 'Social media event', 18, 'format', 327),
       ('EventType', 'talk-science-cafe', 'talk/science cafe', 19, 'format', 193),
       ('EventType', 'training', 'Training', 20, 'format', 207),
       ('EventType', 'webinar', 'Webinar', 21, 'format', 205),
       ('EventType', 'other', 'Other', 22, 'format', 208),

       ('Topic', 'agriculture', 'Agriculture', 0, '', 3),
       ('Topic', 'animals', 'Animals', 1, '', 20),
       ('Topic', 'astronomy-space', 'Astronomy & Space', 2, '', 13),
       ('Topic', 'awards', 'Awards', 3, '', 22),
       ('Topic', 'biology', 'Biology', 4, '', 12),
       ('Topic', 'birds', 'Birds', 5, '', 11),
       ('Topic', 'books-literature', 'Books & Literature', 6, '', 426),
       ('Topic', 'chemistry', 'Chemistry', 7, '', 10),
       ('Topic', 'climate-weather', 'Climate & Weather', 8, '', 27),
       ('Topic', 'computers-technology', 'Computers & Technology', 9, '', 18),
       ('Topic', 'crowd-funding', 'Crowd Funding', 10, '', 7),
       ('Topic', 'disaster-response', 'Disaster Response', 11, '', 203),
       ('Topic', 'ecology-environment', 'Ecology & Environment', 12, '', 9),
       ('Topic', 'education', 'Education', 13, '', 24),
       ('Topic', 'events', 'Events', 14, '', 4),
       ('Topic', 'food', 'Food', 15, '', 25),
       ('Topic', 'geography', 'Geography', 16, '', 2),
       ('Topic', 'geology-earth-science', 'Geology & Earth Science', 17, '', 19),
       ('Topic', 'health-medicine', 'Health & Medicine', 18, '', 23),
       ('Topic', 'history-archeology-culture', 'History, Archeology & Culture', 19, '', 15),
       ('Topic', 'insects-pollinators', 'Insects & Pollinators', 20, '', 16),
       ('Topic', 'nature-outdoors', 'Nature & Outdoors', 21, '', 21),
       ('Topic', 'ocean-water-marine-terrestrial', 'Ocean, Water, Marine & Terrestrial', 22, '', 14),
       ('Topic', 'physics', 'Physics', 23, '', 8),
       ('Topic', 'psychology', 'Psychology', 24, '', 5),
       ('Topic', 'science-policy', 'Science Policy', 25, '', 26),
       ('Topic', 'social-science', 'Social Science', 26, '', 1),
       ('Topic', 'sound', 'Sound', 27, '', 17),
       ('Topic', 'transportation', 'Transportation', 28, '', 6),

       ('Activity', 'on-a-lunch-break', 'On a lunch break', 0, '', 28),
       ('Activity', 'at-sports-stadiums', 'At sports stadiums', 1, '', 29),
       ('Activity', 'at-night', 'At night', 2, '', 30),
       ('Activity', 'at-a-science-center-zoo-or-aquarium', 'At a science center, zoo or aquarium', 3, '', 31),
       ('Activity', 'at-school', 'At school', 4, '', 32),
       ('Activity', 'on-a-walk-run', 'On a walk, run', 5, '', 33),
       ('Activity', 'exclusively-online', 'Exclusively online', 6, '', 34),
       ('Activity', 'at-the-beach', 'At the beach', 7, '', 35),
       ('Activity', 'in-snow-or-rain', 'In snow or rain', 8, '', 36),
       ('Activity', 'at-home', 'At home', 9, '', 37),
       ('Activity', 'in-the-car', 'In the car', 10, '', 38),
       ('Activity', 'emergency-response', 'Emergency Response', 11, '', 39),
       ('Activity', 'on-a-hike', 'On a hike', 12, '', 40),
       ('Activity', 'while-fishing', 'While fishing', 13, '', 41),
       ('Activity', 'in-oceans-streams-rivers-lakes', 'In oceans, streams, rivers, lakes', 14, '', 42),

       ('Audience', 'elementary-school-6-10-years', 'Elementary school (6 - 10 years)', 0, '', 111),
       ('Audience', 'middle-school-11-13-years', 'Middle school (11 - 13 years)', 1, '', 112),
       ('Audience', 'high-school-14-17-years', 'High school (14 - 17 years)', 2, '', 113),
       ('Audience', 'college', 'College', 3, '', 114),
       ('Audience', 'graduate-students', 'Graduate students', 4, '', 115),
       ('Audience', 'adults', 'Adults', 5, '', 116),
       ('Audience', 'families', 'Families', 6, '', 117),
       ('Audience', 'seniors', 'Seniors', 7, '', 118);

-- Each event keeps the one type it had, which events.event_type holds as the
-- name of a variant of the enum that used to list them.
insert into choice_assignments ("target", "list", "value")
select e.event_id, 'EventType', m.slug
from events e
join (
       select column1 as stored, column2 as slug from (values
              ('ActsOfScienceConnected', 'acts-of-science-connected'),
              ('Ambassador', 'ambassador'),
              ('BioBlitz', 'bioblitz'),
              ('CitizenForum', 'citizen-forum'),
              ('CityNatureChallenge', 'city-nature-challenge'),
              ('Conference', 'conference'),
              ('Hackfest', 'hackfest'),
              ('InternationalMonarchMonitoringBlitz', 'international-monarch-monitoring-blitz'),
              ('KidFriendly', 'kid-friendly'),
              ('Library', 'library'),
              ('Megathon', 'megathon'),
              ('MuseumEvent', 'museum-event'),
              ('NatureMonitoring', 'nature-monitoring'),
              ('Online', 'online'),
              ('ProfessionalDevelopment', 'professional-development'),
              ('ScienceFestival', 'science-festival'),
              ('SocialMediaEvent', 'social-media-event'),
              ('TalkOrScienceCafe', 'talk-science-cafe'),
              ('Training', 'training'),
              ('Webinar', 'webinar'),
              ('Other', 'other')
       )
) m on m.stored = e.event_type;

-- Each project keeps the topics, activities and audiences it had, which the
-- tables these replace held by name.
insert or ignore into choice_assignments ("target", "list", "value")
select t.project_id, 'Topic', v.slug
from project_topics t
join choice_values v on v.list = 'Topic' and v.name = t.topic;

insert or ignore into choice_assignments ("target", "list", "value")
select a.project_id, 'Activity', v.slug
from project_activities a
join choice_values v on v.list = 'Activity' and v.name = a.activity;

insert or ignore into choice_assignments ("target", "list", "value")
select a.project_id, 'Audience', v.slug
from project_audiences a
join choice_values v on v.list = 'Audience' and v.name = a.audience;

drop table project_topics;
drop table project_activities;
drop table project_audiences;

-- events_fts indexes the names of an event's types in place of the type
-- column. An FTS5 table's columns cannot be altered, so it is built again.
--
-- Its insert trigger now clears the event's row before writing it. EventAll
-- saves with INSERT OR REPLACE, and a REPLACE does not fire the delete
-- trigger, so each save of an event already here used to add a second row.
drop trigger if exists events_fts_insert;
drop trigger if exists events_fts_update;
drop trigger if exists events_fts_delete;
drop table if exists events_fts;

create virtual table events_fts using fts5(
       event_id unindexed,
       name,
       description,
       presenter,
       location_name,
       location_description,
       address,
       city,
       event_types,
       tags_text,
       tokenize='porter'
);

-- The names of an event's types, as events_fts holds them.
create view events_fts_event_types (event_id, names) as
select target, group_concat(name, ' ' order by position, name)
from choice_names
where list = 'EventType'
group by target;

insert into events_fts (event_id, name, description, presenter, location_name, location_description, address, city, event_types, tags_text)
select event_id, name, description, presenter, location_name, location_description, address, city,
       coalesce((select names from events_fts_event_types t where t.event_id = events.event_id), ''),
       tags_text
from events;

create trigger events_fts_insert after insert on events
begin
    delete from events_fts where event_id = NEW.event_id;
    insert into events_fts (event_id, name, description, presenter, location_name, location_description, address, city, event_types, tags_text)
    values (NEW.event_id, NEW.name, NEW.description, NEW.presenter, NEW.location_name, NEW.location_description, NEW.address, NEW.city,
            coalesce((select names from events_fts_event_types t where t.event_id = NEW.event_id), ''),
            NEW.tags_text);
end;

create trigger events_fts_update after update on events
begin
    update events_fts
    set name = NEW.name,
        description = NEW.description,
        presenter = NEW.presenter,
        location_name = NEW.location_name,
        location_description = NEW.location_description,
        address = NEW.address,
        city = NEW.city,
        tags_text = NEW.tags_text
    where event_id = NEW.event_id;
end;

create trigger events_fts_delete after delete on events
begin
    delete from events_fts where event_id = OLD.event_id;
end;

create trigger events_fts_event_type_assigned after insert on choice_assignments
when NEW.list = 'EventType'
begin
    update events_fts
    set event_types = coalesce((select names from events_fts_event_types t where t.event_id = NEW.target), '')
    where event_id = NEW.target;
end;

create trigger events_fts_event_type_unassigned after delete on choice_assignments
when OLD.list = 'EventType'
begin
    update events_fts
    set event_types = coalesce((select names from events_fts_event_types t where t.event_id = OLD.target), '')
    where event_id = OLD.target;
end;

-- A value arriving after the events that pick it (see above), renamed, or
-- removed, changes what those events are indexed under.
create trigger events_fts_event_type_added after insert on choice_values
when NEW.list = 'EventType'
begin
    update events_fts
    set event_types = coalesce((select names from events_fts_event_types t where t.event_id = events_fts.event_id), '')
    where event_id in (select target from choice_assignments where list = 'EventType' and value = NEW.slug);
end;

create trigger events_fts_event_type_renamed after update of "name" on choice_values
when NEW.list = 'EventType'
begin
    update events_fts
    set event_types = coalesce((select names from events_fts_event_types t where t.event_id = events_fts.event_id), '')
    where event_id in (select target from choice_assignments where list = 'EventType' and value = NEW.slug);
end;

create trigger events_fts_event_type_removed after delete on choice_values
when OLD.list = 'EventType'
begin
    update events_fts
    set event_types = coalesce((select names from events_fts_event_types t where t.event_id = events_fts.event_id), '')
    where event_id in (select target from choice_assignments where list = 'EventType' and value = OLD.slug);
end;

alter table events drop column event_type;

-- Rewind the event sync so orrery is asked for every event again.
--
-- Until this release the sync read an event's types, which orrery sends
-- joined by "||", as the name of a single type, so every event with more than
-- one (and every event of a type hydra did not list) arrived as Other; and it
-- had nowhere to put the topics and audience orrery's events now carry. Those
-- events only come back right if the cursor is moved behind them.
--
-- Replaying the history is safe: events are keyed by orrery's ULIDs and
-- written in place. Deleting the rows restores the defaults the sync falls
-- back to. The misc table is created by common::shared::init, which runs
-- before migrations.
delete from misc where key in (
    'event-sync-bookmark-update',
    'event-sync-bookmark-id'
);
