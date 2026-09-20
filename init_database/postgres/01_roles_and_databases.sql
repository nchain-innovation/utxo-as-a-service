-- Runs once, as the superuser, against an empty PostgreSQL cluster. The
-- official image executes everything in /docker-entrypoint-initdb.d/ only when
-- the data directory is empty, so this never runs against a populated cluster.
--
-- Bootstrap only: roles, databases, ownership. NO schema — the migrations in
-- rust/migrations/ own that, applied by `uaas migrate`. Putting a CREATE TABLE
-- here would put the schema in two places again, which is the problem this
-- whole change exists to remove.
--
-- This is not a port of 01_init.sql. Postgres role and grant semantics differ
-- enough that a translation would be wrong in ways that only show up later.

-- Postgres has no `user@host`: host-based access is pg_hba.conf, and the
-- official image defaults to scram-sha-256 for TCP connections when
-- POSTGRES_PASSWORD is set. There is no equivalent of MySQL's separate
-- 'uaas'@'localhost' and 'uaas'@'%' rows, and no FLUSH PRIVILEGES.
CREATE ROLE uaas LOGIN PASSWORD 'uaas-password';
CREATE ROLE maas LOGIN PASSWORD 'maas-password';

-- Owning the database is what lets the role create its own tables, so the
-- migration runs as the same role that will later read and write. Every table
-- ends up owned by the role that uses it, with no ALTER DEFAULT PRIVILEGES.
--
-- Since PostgreSQL 15 the `public` schema is no longer world-writable — it is
-- owned by pg_database_owner — so database ownership is what grants create
-- rights. GRANT ALL ON DATABASE would NOT be enough: that conveys only
-- CONNECT, CREATE (schema) and TEMP.
--
-- CREATE DATABASE cannot run inside a transaction block. The official
-- entrypoint runs each .sql with `psql -v ON_ERROR_STOP=1` and does not wrap
-- it in one, so this works as written — but it is why these statements cannot
-- be folded into a DO $$ ... $$ block.
CREATE DATABASE uaas_db      OWNER uaas;
CREATE DATABASE main_uaas_db OWNER maas;

-- Passwords are in the repository for local development only.
-- docs/Security.md says so and that stays true.
