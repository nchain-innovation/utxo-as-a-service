-- V11__utxo_autovacuum.sql
-- Reversible:       yes (RESET the same options); storage parameters only.
-- Locks:            SHARE UPDATE EXCLUSIVE on utxo. It does not block reads or
--                   writes, only other ALTER/VACUUM/ANALYZE on the same table.
-- Rewrites tables:  no. Storage parameters are catalogue metadata.
-- In-flight writes: unaffected — they continue while this runs.

-- CS-421 turns `utxo` into a table with constant churn: a row is inserted when
-- a monitored output appears and deleted when the settle moves it to
-- utxo_spent. PostgreSQL does not overwrite a deleted row, it marks the tuple
-- dead and leaves it for autovacuum to reclaim.
--
-- The defaults are wrong for that shape. Autovacuum fires when
--     dead tuples > autovacuum_vacuum_threshold
--                 + autovacuum_vacuum_scale_factor * reltuples
-- and the default scale factor is 0.2, so the table carries 20% dead tuples
-- before anything reclaims them. That is a feedback loop rather than a steady
-- state: the bloat makes each vacuum scan more pages, which makes it slower,
-- which allows more bloat to accumulate before the next one finishes.
--
-- 0.02 with a 10000-row floor means a 5,000,000-row table vacuums at ~100,000
-- dead tuples instead of ~1,000,000, and a small table is not vacuumed
-- constantly while it is still small.
ALTER TABLE utxo SET (
    autovacuum_vacuum_scale_factor = 0.02,
    autovacuum_vacuum_threshold    = 10000
);

-- Deliberately NOT applied to utxo_spent. Its rows are inserted once and
-- updated once by the settle, so it produces one dead tuple per row rather
-- than churning, and the fillfactor = 85 in V6 exists to keep that update
-- heap-only. The default scale factor is a reasonable fit for that; overriding
-- it here would be tuning without a measurement behind it.
