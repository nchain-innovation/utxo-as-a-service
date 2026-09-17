-- V5__utxo.sql
-- Reversible:       yes (DROP TABLE); this runs against an empty database.
-- Locks:            none — creates only, no existing object is touched.
-- Rewrites tables:  no.
-- In-flight writes: impossible; the service refuses to start until the schema
--                   version it expects has been applied.

-- Presence in this table IS spendability. There is no `spent` column: a spent
-- output moves to utxo_spent. That makes the common query — what can this
-- identifier spend — a scan of live rows only, and removes the sentinel-value
-- problem the MariaDB schema had.
CREATE TABLE utxo (
    txid           bytea   NOT NULL,
    vout           integer NOT NULL,
    satoshis       bigint  NOT NULL,
    locking_script bytea   NOT NULL,
    -- The bytes the matching pattern captured in its `identifier` group. NULL
    -- is legitimate: a pattern need not declare one.
    identifier     bytea,
    -- NULL means the output was created by a transaction that is not yet in a
    -- block, so it has no height.
    created_height integer,
    PRIMARY KEY (txid, vout)
);

CREATE INDEX idx_utxo_identifier ON utxo (identifier);

-- BRIN, not btree. created_height is correlated with physical order because
-- rows arrive in height order, which is exactly the case BRIN is for: a few
-- pages of summary instead of an index the size of the table.
CREATE INDEX idx_utxo_created_brin ON utxo USING brin (created_height);
