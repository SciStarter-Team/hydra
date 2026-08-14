-- Rewind the entity list sync so orrery is asked for every list again.
--
-- Passes before this release stamped the current time onto lists that
-- arrived with a null creation date, then advanced the bookmark to the
-- highest creation date they had seen. Orrery sorts its null dates first,
-- so that happened on the first pass and left the cursor in the future,
-- after which every request asked for lists created since "now" and got
-- nothing back. The lists skipped while it was stuck only arrive if the
-- cursor is moved back behind them.
--
-- Deleting the rows rather than assigning them restores the defaults the
-- sync itself falls back to. The misc table is created by
-- common::shared::init, which runs before migrations.
--
-- Replaying the full history is only safe as of this release: entries now
-- carry stable ids drawn from entity_list_entry_id_map, so a list that is
-- fetched twice has its entries replaced in place and any entry orrery no
-- longer sends swept away. Before that, a second pass inserted a fresh
-- duplicate of every entry on every list.
DELETE FROM misc WHERE key IN (
    'entity-list-sync-bookmark-update',
    'entity-list-sync-bookmark-id'
);
