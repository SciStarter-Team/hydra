# Entities FTS Integration Guide

This guide explains how to add new entity types to the hybrid FTS5 search system.

## Overview

The entities system uses a **hybrid FTS approach**:
- **Unified search**: `entities_fts` table for cross-type "search everything" functionality
- **Type-specific search**: Individual `*_fts` tables (e.g., `projects_fts`, `events_fts`) for optimized searches
- **Smart summaries**: Content + tags combined into searchable summaries

## Architecture

```
entities table (slug, kind, target) 
    ↓
entities_fts table (unified search)
    ↓
Individual *_fts tables (type-specific search)
```

## Adding a New Entity Type

To add a new entity type (e.g., "Html", "Forum", "User"), you need to create **two new migrations**:

### 1. Create Type-Specific FTS Table (Optional but Recommended)

Create a migration for the dedicated FTS table:

```bash
cargo sqlx migrate add create_htmls_fts
```

```sql
-- Migration: create_htmls_fts.sql
CREATE VIRTUAL TABLE htmls_fts USING fts5(
    html_id UNINDEXED,
    title,
    content_text,
    meta_description,
    tags_text,
    content='htmls',
    content_rowid='rowid'
);

-- Populate and create triggers following existing patterns
-- (See projects_fts and events_fts migrations for examples)
```

### 2. Integrate with Unified entities_fts System

Create a migration to add the new entity type to the unified system:

```bash
cargo sqlx migrate add add_html_entities_to_fts
```

This migration must handle **four components**:

#### A. Initial Population

```sql
-- Populate entities_fts with existing HTML entities
INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
SELECT 
    e.slug,
    'Html' as entity_type,  -- Match the entities.kind exactly
    h.html_id,
    h.title,
    h.title || ': ' || 
    CASE 
        WHEN LENGTH(h.content_text) > 200 THEN SUBSTR(h.content_text, 1, 197) || '...'
        ELSE h.content_text
    END ||
    CASE 
        WHEN LENGTH(h.tags_text) > 0 THEN '. Tags: ' || h.tags_text
        ELSE ''
    END as summary
FROM entities e
JOIN htmls h ON e.target = h.html_id
WHERE e.kind = 'Html';  -- Must match entities.kind case exactly
```

#### B. Recreate Entity INSERT Trigger

**Important**: You must DROP and CREATE the entire trigger, not edit it:

```sql
-- Drop existing trigger
DROP TRIGGER IF EXISTS entities_fts_entity_insert;

-- Recreate with new entity type added
CREATE TRIGGER IF NOT EXISTS entities_fts_entity_insert AFTER INSERT ON entities
BEGIN
    -- Handle project entities (copy existing logic)
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT ... FROM projects p WHERE NEW.kind = 'Project' AND ...;
    
    -- Handle event entities (copy existing logic) 
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT ... FROM events e WHERE NEW.kind = 'Event' AND ...;
    
    -- Add NEW entity type
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT 
        NEW.slug, 'Html', h.html_id, h.title,
        h.title || ': ' || SUBSTR(h.content_text, 1, 200) ||
        CASE WHEN LENGTH(h.tags_text) > 0 THEN '. Tags: ' || h.tags_text ELSE '' END
    FROM htmls h 
    WHERE NEW.kind = 'Html' AND h.html_id = NEW.target;
END;
```

#### C. Recreate Entity UPDATE Trigger

```sql
-- Drop existing trigger
DROP TRIGGER IF EXISTS entities_fts_entity_update;

-- Recreate with new entity type (copy all existing logic + add new type)
CREATE TRIGGER IF NOT EXISTS entities_fts_entity_update AFTER UPDATE ON entities
BEGIN
    DELETE FROM entities_fts WHERE slug = OLD.slug;
    
    -- Re-insert all entity types including new one
    -- (Same INSERT logic as above)
END;
```

#### D. Create Content UPDATE Trigger

```sql
-- Create new trigger for HTML content changes
CREATE TRIGGER IF NOT EXISTS entities_fts_html_update
AFTER UPDATE OF title, content_text, tags_text ON htmls
BEGIN
    DELETE FROM entities_fts WHERE entity_type = 'Html' AND entity_id = NEW.html_id;
    
    INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
    SELECT 
        e.slug, 'Html', NEW.html_id, NEW.title,
        NEW.title || ': ' || SUBSTR(NEW.content_text, 1, 200) ||
        CASE WHEN LENGTH(NEW.tags_text) > 0 THEN '. Tags: ' || NEW.tags_text ELSE '' END
    FROM entities e 
    WHERE e.kind = 'Html' AND e.target = NEW.html_id;
END;
```

## Summary Format Guidelines

Create meaningful, searchable summaries using this pattern:

```sql
-- Projects: "Name by Presenter: Description. Tags: tag1 tag2"
project_name || ' by ' || presenter || ': ' || description || '. Tags: ' || tags_text

-- Events: "Name at Location: Description. Tags: tag1 tag2" 
event_name || ' at ' || location || ': ' || description || '. Tags: ' || tags_text

-- HTML: "Title: Content excerpt. Tags: tag1 tag2"
title || ': ' || content_excerpt || '. Tags: ' || tags_text

-- Users: "Display Name: Bio excerpt. Interests: interest1 interest2"
display_name || ': ' || bio_excerpt || '. Interests: ' || interests
```

**Key principles:**
- Include the most identifying information first
- Truncate long content (~200 chars max)
- Always include tags/categories for searchability
- Use natural language that users would search for

## Migration Template

Use this template for adding new entity types:

```sql
-- Migration: add_[entity_type]_to_entities_fts.sql

-- Step 1: Add initial population
INSERT INTO entities_fts(slug, entity_type, entity_id, title, summary)
SELECT 
    e.slug, '[EntityType]', t.id, t.title,
    -- Create meaningful summary here
FROM entities e
JOIN [table_name] t ON e.target = t.id
WHERE e.kind = '[EntityType]';

-- Step 2: Recreate entity INSERT trigger with new type
DROP TRIGGER IF EXISTS entities_fts_entity_insert;
CREATE TRIGGER IF NOT EXISTS entities_fts_entity_insert AFTER INSERT ON entities
BEGIN
    -- Copy ALL existing entity type logic from current trigger
    -- Add new entity type logic
END;

-- Step 3: Recreate entity UPDATE trigger with new type  
DROP TRIGGER IF EXISTS entities_fts_entity_update;
CREATE TRIGGER IF NOT EXISTS entities_fts_entity_update AFTER UPDATE ON entities
BEGIN
    -- Copy ALL existing entity type logic from current trigger
    -- Add new entity type logic
END;

-- Step 4: Create content update trigger
CREATE TRIGGER IF NOT EXISTS entities_fts_[entity_type]_update
AFTER UPDATE OF [relevant_fields] ON [table_name]
BEGIN
    DELETE FROM entities_fts WHERE entity_type = '[EntityType]' AND entity_id = NEW.id;
    INSERT INTO entities_fts(...) SELECT ... FROM entities e WHERE ...;
END;
```

## Finding Current Trigger Logic

To get the current trigger definitions for copying:

```sql
-- View current triggers
.schema entities_fts_entity_insert
.schema entities_fts_entity_update

-- Or check the latest migration files that created/modified these triggers
```

## Testing Your Integration

After applying your migrations:

```sql
-- 1. Check population
SELECT entity_type, COUNT(*) FROM entities_fts GROUP BY entity_type;

-- 2. Test search
SELECT slug, entity_type, title 
FROM entities_fts 
WHERE entities_fts MATCH 'your_test_term' 
LIMIT 5;

-- 3. Test content updates
UPDATE htmls SET title = 'New Title' WHERE html_id = 'test_id';
-- Check that entities_fts was updated automatically
```

## Best Practices

1. **Case sensitivity**: Match `entities.kind` exactly (usually PascalCase)
2. **Entity type naming**: Use consistent casing in `entity_type` field  
3. **Summary length**: Keep summaries under 300 characters total
4. **Tag integration**: Always include tags when available
5. **Trigger recreation**: Always DROP and CREATE entire triggers, never edit
6. **Copy existing logic**: When recreating triggers, copy ALL existing entity types
7. **Test thoroughly**: Verify population, search, and content updates
8. **Documentation**: Update this guide when adding new patterns

## Troubleshooting

**Empty entities_fts after migration?**
- Check case sensitivity in WHERE clauses
- Verify JOIN conditions match actual data
- Test individual SELECT queries before running migration

**Search not finding results?**
- Check FTS5 syntax (use MATCH operator)
- Verify summary content includes searchable terms
- Test with simple terms first

**Triggers not firing after recreation?**
- Verify all existing entity types were copied correctly
- Check trigger names for conflicts
- Confirm UPDATE OF clause includes all relevant fields