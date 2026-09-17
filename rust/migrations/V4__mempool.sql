-- V4__mempool.sql
-- Reversible:       yes (DROP TABLE); this runs against an empty database.
-- Locks:            none — creates only, no existing object is touched.
-- Rewrites tables:  no.
-- In-flight writes: impossible; the service refuses to start until the schema
--                   version it expects has been applied.

CREATE TABLE mempool (
    hash     bytea       PRIMARY KEY,
    locktime bigint      NOT NULL,
    fee      bigint      NOT NULL,
    -- Was `int unsigned`, a unix timestamp that overflows in 2106. timestamptz
    -- stores an absolute instant; plain timestamp would discard the offset.
    seen_at  timestamptz NOT NULL DEFAULT now(),
    -- Raw bytes, not hex in a longtext: half the size, and TOASTed by Postgres
    -- automatically once it exceeds the page threshold.
    tx       bytea       NOT NULL
);

-- Startup reads the mempool ordered by age. Without this it is a filesort over
-- the whole table every time the service comes up.
CREATE INDEX idx_mempool_seen_at ON mempool (seen_at);
