-- V1__blocks.sql
-- Reversible:       yes (DROP TABLE); this runs against an empty database.
-- Locks:            none — creates only, no existing object is touched.
-- Rewrites tables:  no.
-- In-flight writes: impossible; the service refuses to start until the schema
--                   version it expects has been applied.

CREATE TABLE blocks (
    hash         bytea    PRIMARY KEY,
    height       integer  NOT NULL,
    version      integer  NOT NULL,
    prev_hash    bytea    NOT NULL,
    merkle_root  bytea    NOT NULL,
    -- Named block_time, not timestamp: a column named after a type reads badly
    -- and has to be quoted in some contexts.
    block_time   integer  NOT NULL,
    bits         integer  NOT NULL,
    nonce        integer  NOT NULL,
    -- Renamed from `offset`, which is a reserved word in both dialects and had
    -- to be backtick-quoted everywhere it appeared.
    file_offset  bigint   NOT NULL,
    blocksize    integer  NOT NULL,
    numtxs       integer  NOT NULL,
    -- Height was indexed but not unique. Two rows at one height is not a state
    -- this service can be in, so the database should say so.
    CONSTRAINT uq_blocks_height UNIQUE (height)
);
