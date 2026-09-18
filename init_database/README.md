# Database initialisation

Two engines live here while the Python REST API is still on MariaDB.

| Path | Engine | Runs |
|---|---|---|
| `postgres/01_roles_and_databases.sql` | PostgreSQL | mounted into the `postgres` service |
| `01_init.sql`, `02_ensure_grants.sql` | MariaDB | mounted into the `database` service |

The PostgreSQL script is the one the Rust service depends on. The MariaDB
scripts remain only because the Python REST API still reads that database.

## Where the schema lives

**Not here, and not in the application code.** `rust/migrations/` holds the
versioned DDL, applied by `uaas migrate`. These scripts create roles and
databases and nothing else.

That split matters. Everything in this directory runs only when the container's
data directory is empty, so it cannot be used to evolve a schema — a change
made here would never reach a database that already exists. Migrations can,
and they record what they have applied.

Both MariaDB scripts go away once the Python REST API reads from PostgreSQL
too.
