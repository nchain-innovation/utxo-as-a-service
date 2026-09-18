-- V10__connect.sql
-- Reversible:       yes (DROP TABLE); this runs against an empty database.
-- Locks:            none — creates only, no existing object is touched.
-- Rewrites tables:  no.
-- In-flight writes: impossible; the service refuses to start until the schema
--                   version it expects has been applied.

CREATE TABLE connect (
    id         bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    -- Was a formatted VARCHAR(64), which could not be range-queried or sorted
    -- without parsing every row.
    event_time timestamptz NOT NULL DEFAULT now(),
    ip         inet        NOT NULL,
    event      text        NOT NULL
);

CREATE INDEX idx_connect_event_time ON connect (event_time);
