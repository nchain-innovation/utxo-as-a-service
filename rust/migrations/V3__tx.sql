-- V3__tx.sql
-- Reversible:       yes (DROP TABLE); this runs against an empty database.
-- Locks:            none — creates only, no existing object is touched.
-- Rewrites tables:  no.
-- In-flight writes: impossible; the service refuses to start until the schema
--                   version it expects has been applied.

CREATE TABLE tx (
    hash       bytea   PRIMARY KEY,
    height     integer NOT NULL,
    blockindex integer NOT NULL,
    txsize     integer NOT NULL,
    satoshis   bigint  NOT NULL
);

CREATE INDEX idx_tx_height_blockindex ON tx (height, blockindex);
