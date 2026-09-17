-- V6__utxo_spent.sql
-- Reversible:       yes (DROP TABLE); this runs against an empty database.
-- Locks:            none — creates only, no existing object is touched.
-- Rewrites tables:  no.
-- In-flight writes: impossible; the service refuses to start until the schema
--                   version it expects has been applied.

-- fillfactor is load-bearing here, not tuning. Rows in this table are inserted
-- once and updated exactly once, when the spend is mined and spent_height is
-- filled in. A heap-only-tuple update needs free space on the same page; at the
-- default fillfactor = 100 there is none, so HOT never fires and every settle
-- writes a new page plus every index entry.
CREATE TABLE utxo_spent (
    txid           bytea   NOT NULL,
    vout           integer NOT NULL,
    satoshis       bigint  NOT NULL,
    locking_script bytea   NOT NULL,
    identifier     bytea,
    created_height integer,
    spent_txid     bytea   NOT NULL,
    -- NULL means the spend has been seen but not yet mined.
    spent_height   integer,
    PRIMARY KEY (txid, vout)
) WITH (fillfactor = 85);

CREATE INDEX idx_spent_identifier ON utxo_spent (identifier);

-- Both BRIN for the same reason as utxo, plus one that matters more here: a
-- summarising index does not block a HOT update (PostgreSQL 16+), whereas a
-- btree on spent_height would take the HOT rate to zero — the column the
-- settle updates cannot be the column a btree indexes without losing HOT.
CREATE INDEX idx_spent_created_brin ON utxo_spent USING brin (created_height);
CREATE INDEX idx_spent_height_brin  ON utxo_spent USING brin (spent_height);
