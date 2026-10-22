-- A page's last change is now read from `log.last_changed` under
-- `last_changed:` keys; the rows cached from the old `log.last_edited`
-- would never be read again.
DELETE FROM lazy_cache WHERE cache_key LIKE 'last_edited:%';
