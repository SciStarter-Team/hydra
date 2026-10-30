# Entities FTS Guide

`entities_fts` is the FTS5 index behind the finder's text search and the header
search box. This guide explains how it is kept up to date and how to change what
it indexes.

## Overview

Each published entity has one row in `entities_fts`:

| Column        | Indexed | Contents                                           |
|---------------|---------|----------------------------------------------------|
| `slug`        | no      | the entity's slug, used to join back to `entities` |
| `entity_type` | no      | the entity's kind (`Project`, `Event`, `Wysiwyg`…) |
| `entity_id`   | no      | the entity's target (project id, event id…)        |
| `title`       | yes     | the entity's name                                  |
| `summary`     | yes     | everything else that should be searchable          |

`summary` is never displayed. It is the searchable text of the entity, with
one field per line, so add a field to it rather than adding a column.

The separate `projects_fts` and `events_fts` tables serve the exchange and admin
searches and are maintained by their own triggers.

## One definition: `entities_fts_documents`

The view `entities_fts_documents` defines the row every published entity should
have, one `UNION ALL` arm per kind. Every trigger that maintains the index
deletes the affected rows and re-inserts them from the view:

- `entities` insert, update and delete refresh by slug.
- `projects`, `events` and `pagecontent` insert, update and delete refresh by
  entity type and id. Their update triggers have no column list, so any column
  the view reads refreshes the entry.

Two helper views feed it:

- `entities_fts_descriptions` gives the plain text of project and event
  descriptions. The detail-page editor stores Quill Delta JSON, whose text is
  the `insert` strings of its ops. Other formats are indexed as stored.
- `entities_fts_project_hosts` gives the host of each project's URL, which
  names the platform the project runs on (zooniverse.org, inaturalist.org).

## Changing what is indexed

Write a migration that drops and recreates `entities_fts_documents` with the
change, then rebuilds the index:

```sql
DROP VIEW entities_fts_documents;
CREATE VIEW entities_fts_documents (slug, entity_type, entity_id, title, summary) AS
...;

DELETE FROM entities_fts;
INSERT INTO entities_fts (slug, entity_type, entity_id, title, summary)
SELECT slug, entity_type, entity_id, title, summary FROM entities_fts_documents;
```

The triggers read the view when they fire, so they need no change.

## Adding an entity kind

1. Add an arm to `entities_fts_documents` for the new kind. Match
   `entities.kind` exactly (PascalCase). Keep the arm's
   `e.kind NOT IN ('Redirect', 'WordPress', 'Placeholder')` term, which lets
   lookups by target use the partial index `entities_by_target`.
2. If the kind's content lives in a new table, add insert, update and delete
   triggers on that table following the `projects` ones: delete the kind's rows
   for the old id, then insert from the view for the new id.
3. Rebuild the index as above.

## Testing

```sql
-- One row per published entity, no duplicates
SELECT entity_type, COUNT(*) FROM entities_fts GROUP BY entity_type;
SELECT slug FROM entities_fts GROUP BY slug HAVING COUNT(*) > 1;

-- Search the way the finder does (a quoted prefix phrase)
SELECT slug, entity_type, title FROM entities_fts
WHERE entities_fts MATCH '"zooniverse"*' LIMIT 5;

-- Content edits refresh the entry
UPDATE projects SET goal = 'quuxword' WHERE project_id = '...';
SELECT slug FROM entities_fts WHERE entities_fts MATCH 'quuxword';
```
