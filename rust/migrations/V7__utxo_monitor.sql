-- V7__utxo_monitor.sql
-- Reversible:       yes (DROP TABLE); this runs against an empty database.
-- Locks:            none — creates only, no existing object is touched.
-- Rewrites tables:  no.
-- In-flight writes: impossible; the service refuses to start until the schema
--                   version it expects has been applied.

-- Which monitors matched an outpoint. Many-to-many, so it cannot live as a
-- column on either utxo table.
CREATE TABLE utxo_monitor (
    txid    bytea   NOT NULL,
    vout    integer NOT NULL,
    monitor text    NOT NULL,
    PRIMARY KEY (txid, vout, monitor)
    -- No foreign key. The outpoint lives in whichever of utxo or utxo_spent
    -- currently holds it, and a foreign key cannot express "one of these two".
);

CREATE INDEX idx_utxo_monitor_name ON utxo_monitor (monitor);
