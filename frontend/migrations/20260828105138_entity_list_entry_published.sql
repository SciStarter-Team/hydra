-- Whether an entry is shown on the list's page.
--
-- A list is how a curated page hands a set of projects to a group of people,
-- and curating one means drafting it: staff put a project on a page before the
-- page goes out, and take one off without losing the note and link that went
-- with it. orrery has this as `LibraryKit.published` ("The kit will only be
-- visible on the site if this box is checked"); hydra's lists had no
-- equivalent, so an entry could only be present or deleted.
--
-- Existing entries default to published, because that is what they have been
-- doing: every entry on every list is currently visible, and a default of 0
-- would empty every list at once.
ALTER TABLE entity_list_entries ADD COLUMN published INTEGER NOT NULL DEFAULT 1;

-- The list page and the staff report both ask a list for its published
-- entries in priority order.
CREATE INDEX IF NOT EXISTS idx_entity_list_entries_published
   ON entity_list_entries (list_id, published, priority);
