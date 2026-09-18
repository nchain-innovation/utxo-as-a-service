-- V2__orphans.sql
-- Reversible:       yes (DROP TABLE); this runs against an empty database.
-- Locks:            none — creates only, no existing object is touched.
-- Rewrites tables:  no.
-- In-flight writes: impossible; the service refuses to start until the schema
--                   version it expects has been applied.

CREATE TABLE orphans (
    -- The MariaDB table had no primary key at all, so the same orphan could be
    -- recorded twice.
    hash        bytea       PRIMARY KEY,
    height      integer     NOT NULL,
    version     integer     NOT NULL,
    prev_hash   bytea       NOT NULL,
    merkle_root bytea       NOT NULL,
    block_time  integer     NOT NULL,
    bits        integer     NOT NULL,
    nonce       integer     NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);
