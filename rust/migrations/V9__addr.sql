-- V9__addr.sql
-- Reversible:       yes (DROP TABLE); this runs against an empty database.
-- Locks:            none — creates only, no existing object is touched.
-- Rewrites tables:  no.
-- In-flight writes: impossible; the service refuses to start until the schema
--                   version it expects has been applied.

CREATE TABLE addr (
    -- inet is a native Postgres type: it validates the address on insert,
    -- stores IPv4 in 7 bytes and IPv6 in 19, and supports subnet operators.
    -- It has no MariaDB equivalent, which is why this column was an unindexed
    -- `text` with no primary key.
    ip        inet        PRIMARY KEY,
    -- Was `int`, which cannot hold the u64 service bitfield the protocol
    -- defines; values above 2^31 were rejected outright.
    services  bigint      NOT NULL,
    port      integer     NOT NULL,
    last_seen timestamptz NOT NULL DEFAULT now()
);
