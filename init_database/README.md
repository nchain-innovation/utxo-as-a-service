# Database initialisation

Two engines live here while the PostgreSQL migration is in progress.

| Path | Engine | Runs |
|---|---|---|
| `01_init.sql`, `02_ensure_grants.sql` | MariaDB | mounted into the `database` service |
| `postgres/01_roles_and_databases.sql` | PostgreSQL | mounted into the `postgres` service |

The MariaDB scripts are what the service uses today. The PostgreSQL script is
inert: the `postgres` service is defined so the schema and the migration runner
can be exercised, but nothing reads from it yet.

## Where the schema lives

**Not here, and not in the application code.** `rust/migrations/` holds the
versioned DDL, applied by `uaas migrate`. These scripts create roles and
databases and nothing else.

That split matters. Everything in this directory runs only when the container's
data directory is empty, so it cannot be used to evolve a schema — a change
made here would never reach a database that already exists. Migrations can,
and they record what they have applied.

Both MariaDB scripts go away with the driver, once the service reads from
PostgreSQL.
