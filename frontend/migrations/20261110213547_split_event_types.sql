-- An event's format, the program or campaign it is part of, and the kind of
-- venue it is held at are three questions, each with a list of its own,
-- where until now they were one list of event types whose values each said
-- which question they answered. See frontend/src/choice.rs.
--
-- Each event type moves to the list of the question it answers, keeping its
-- slug, and what picks it moves with it. Kid-friendly, the one value that
-- answers none of them, is gone: orrery reads it as an audience now, and the
-- event sync, rewound by the migration before this one, brings each event's
-- audience from orrery.

drop trigger events_fts_event_type_assigned;
drop trigger events_fts_event_type_unassigned;
drop trigger events_fts_event_type_added;
drop trigger events_fts_event_type_renamed;
drop trigger events_fts_event_type_removed;
drop view events_fts_event_types;
drop view choice_names;

delete from choice_assignments
where list = 'EventType'
  and value in (select slug from choice_values where list = 'EventType' and facet = '');

delete from choice_values where list = 'EventType' and facet = '';

update choice_assignments
set list = (
       select case v.facet
              when 'format' then 'EventFormat'
              when 'program' then 'EventProgram'
              when 'venue' then 'EventVenue'
       end
       from choice_values v
       where v.list = 'EventType' and v.slug = choice_assignments.value
)
where list = 'EventType'
  and value in (select slug from choice_values where list = 'EventType');

-- A pick of a value that was never here has no question to go to.
delete from choice_assignments where list = 'EventType';

update choice_values
set list = case facet
       when 'format' then 'EventFormat'
       when 'program' then 'EventProgram'
       when 'venue' then 'EventVenue'
end
where list = 'EventType';

-- Each list keeps the order it had; the positions skip those of the values
-- now in the others, which order does not mind.

-- What each project and event picks, with the names and positions of the
-- values: the one place the two tables are joined, for everything that shows
-- or searches by them.
create view choice_names (target, list, value, name, position) as
select a.target, a.list, a.value, v.name, v.position
from choice_assignments a
join choice_values v on v.list = a.list and v.slug = a.value;

-- The lists that together say what kind of event an event is, in the order
-- they are asked, for whatever shows or searches by all of an event's types
-- at once. ChoiceList::EVENT_TYPES in frontend/src/choice.rs names the same
-- lists in the same order.
create view event_type_lists (list, rank) as
values ('EventFormat', 0), ('EventProgram', 1), ('EventVenue', 2);

-- What each event picks from those lists, ordered by `rank`, then
-- `position`, then `name`.
create view event_type_names (target, list, value, name, position, rank) as
select n.target, n.list, n.value, n.name, n.position, l.rank
from choice_names n
join event_type_lists l on l.list = n.list;

-- The names of an event's types, as events_fts holds them.
create view events_fts_event_types (event_id, names) as
select target, group_concat(name, ' ' order by rank, position, name)
from event_type_names
group by target;

-- Last, once nothing reads it: the views are made again above, and dropping
-- a column checks every view and trigger still makes sense without it.
alter table choice_values drop column facet;

update events_fts
set event_types = coalesce((select names from events_fts_event_types t where t.event_id = events_fts.event_id), '');

create trigger events_fts_event_type_assigned after insert on choice_assignments
when NEW.list in (select list from event_type_lists)
begin
    update events_fts
    set event_types = coalesce((select names from events_fts_event_types t where t.event_id = NEW.target), '')
    where event_id = NEW.target;
end;

create trigger events_fts_event_type_unassigned after delete on choice_assignments
when OLD.list in (select list from event_type_lists)
begin
    update events_fts
    set event_types = coalesce((select names from events_fts_event_types t where t.event_id = OLD.target), '')
    where event_id = OLD.target;
end;

-- A value arriving after the events that pick it, renamed, or removed,
-- changes what those events are indexed under. A value moving from one of
-- the lists to another does not: the names are indexed together.
create trigger events_fts_event_type_added after insert on choice_values
when NEW.list in (select list from event_type_lists)
begin
    update events_fts
    set event_types = coalesce((select names from events_fts_event_types t where t.event_id = events_fts.event_id), '')
    where event_id in (select target from choice_assignments where list = NEW.list and value = NEW.slug);
end;

create trigger events_fts_event_type_renamed after update of "name" on choice_values
when NEW.list in (select list from event_type_lists)
begin
    update events_fts
    set event_types = coalesce((select names from events_fts_event_types t where t.event_id = events_fts.event_id), '')
    where event_id in (select target from choice_assignments where list = NEW.list and value = NEW.slug);
end;

create trigger events_fts_event_type_removed after delete on choice_values
when OLD.list in (select list from event_type_lists)
begin
    update events_fts
    set event_types = coalesce((select names from events_fts_event_types t where t.event_id = events_fts.event_id), '')
    where event_id in (select target from choice_assignments where list = OLD.list and value = OLD.slug);
end;
