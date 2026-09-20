-- V13__mempool_seen_height.sql
-- Reversible:       yes (DROP COLUMN, DROP INDEX).
-- Locks:            ACCESS EXCLUSIVE on mempool, held only for the catalogue
--                   update; then SHARE for the index build, which blocks
--                   writes to mempool but not reads.
-- Rewrites tables:  no. Verified against postgres:17 by watching
--                   pg_class.relfilenode across the statement: unchanged by
--                   ADD COLUMN, changed by an ALTER COLUMN TYPE on the same
--                   table. Existing rows are untouched and read back NULL.
-- In-flight writes: impossible; the service refuses to start until the schema
--                   version it expects has been applied.

-- mempool already records `seen_at`, but eviction is measured in blocks (see
-- V12), and the two disagree exactly when it matters: after an outage,
-- wall-clock age has advanced and height has not.
--
-- NULL on rows written before this migration. They are evictable: a row
-- predating the feature has by definition survived a restart, which is longer
-- than any threshold worth setting.
ALTER TABLE mempool ADD COLUMN seen_height integer;

CREATE INDEX idx_mempool_seen_height ON mempool (seen_height);
