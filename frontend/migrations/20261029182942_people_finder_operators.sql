-- Everyone who operates a published project or event: its page's creator and
-- its named editors, the people `edit_access::may_edit` names. These are the
-- members of the managers group of a published entity in orrery, which is
-- how its People Finder (messaging/api.py, people_finder_audience) tells an
-- operator from a participant, and who it lets send.
--
-- One definition for both of the People Finder's questions -- which of the
-- people in an area are operators, and whether the sender is one -- so the two
-- cannot drift apart.

CREATE VIEW "published_entity_operators" AS
SELECT p."page_creator_id" AS "user_id"
  FROM "projects" p
  JOIN "entities" e ON e."target" = p."project_id"
 WHERE e."publish" = 1 AND p."page_creator_id" != ''
UNION
SELECT pe."user_id"
  FROM "project_page_editors" pe
  JOIN "entities" e ON e."target" = pe."project_id"
 WHERE e."publish" = 1
UNION
SELECT v."page_creator_id"
  FROM "events" v
  JOIN "entities" e ON e."target" = v."event_id"
 WHERE e."publish" = 1 AND v."page_creator_id" != ''
UNION
SELECT ve."user_id"
  FROM "event_page_editors" ve
  JOIN "entities" e ON e."target" = ve."event_id"
 WHERE e."publish" = 1;
