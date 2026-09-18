-- V8__collection.sql
-- Reversible:       yes (DROP TABLE); this runs against an empty database.
-- Locks:            none — creates only, no existing object is touched.
-- Rewrites tables:  no.
-- In-flight writes: impossible; the service refuses to start until the schema
--                   version it expects has been applied.

CREATE TABLE collection (
    hash    bytea NOT NULL,
    -- Named monitor, not name: it holds a monitor's name and `name` said
    -- nothing about which name.
    monitor text  NOT NULL,
    tx      bytea NOT NULL,
    PRIMARY KEY (hash, monitor)
);

-- The MariaDB schema also carried an index on (hash, name), which duplicated
-- the primary key's leading columns and was never the index a query wanted.
-- Queries filter by monitor name alone.
CREATE INDEX idx_collection_monitor ON collection (monitor);
