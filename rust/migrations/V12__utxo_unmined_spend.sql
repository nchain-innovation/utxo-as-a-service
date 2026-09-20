-- V12__utxo_unmined_spend.sql
-- Reversible:       yes (DROP TABLE).
-- Locks:            none on any existing table — this creates only.
-- Rewrites tables:  no.
-- In-flight writes: impossible; the service refuses to start until the schema
--                   version it expects has been applied.

-- The set of spends seen but not yet mined, so that CS-423 can find the ones
-- that never will be. `utxo_spent.spent_height IS NULL` already means exactly
-- this, so the obvious schema was an index on utxo_spent supporting that
-- predicate. Measured against postgres:17, that index costs too much:
--
--   table         indexes                                    HOT updates
--   ------------  -----------------------------------------  -----------
--   control       none                                          16.0%
--   candidate A   btree (spent_seen_height)                     16.0%
--   candidate B   btree (spent_seen_height) WHERE
--                 spent_height IS NULL                           0.0%
--
-- A partial index counts its *predicate* columns as indexed, so B makes the
-- settle's `UPDATE ... SET spent_height` modify an indexed column and no
-- update on that table can be heap-only again. That is precisely what V6's
-- `fillfactor = 85` exists to protect, and it would be silently undone.
--
-- Candidate A keeps HOT, but its range scan cannot separate unmined rows from
-- the settled ones that share the column, so it degrades as utxo_spent grows.
--
-- A side table avoids the choice. utxo_spent is not altered at all — no new
-- column, no new index, no lock, HOT untouched — and the eviction scan runs
-- over a table holding only the unconfirmed spends, which is bounded by the
-- mempool rather than by the chain.
CREATE TABLE utxo_unmined_spend (
    txid        bytea   NOT NULL,
    vout        integer NOT NULL,
    -- The chain tip when the spend was first seen. Height, not a timestamp:
    -- wall-clock age keeps advancing while the service is stopped or not
    -- syncing, so a restart after an outage would reclaim everything at once.
    -- Height only advances when the chain does.
    seen_height integer NOT NULL,
    -- Keyed on the outpoint, never the spending txid, for the same reason the
    -- settle in V6 is: a spend announced under one txid and mined as a
    -- malleated sibling is the same spend.
    PRIMARY KEY (txid, vout)
);

-- The eviction query is a range over seen_height. Plain btree, not BRIN:
-- rows are deleted as spends confirm, so physical order stops tracking
-- seen_height and the block-range summary BRIN depends on decays.
CREATE INDEX idx_unmined_seen_height ON utxo_unmined_spend (seen_height);
