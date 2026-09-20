# Database initialisation

One engine, one script.

| Path | Engine | Runs |
|---|---|---|
| `postgres/01_roles_and_databases.sql` | PostgreSQL | mounted into the `postgres` service |

The MariaDB scripts are gone: nothing speaks MySQL any more, on either side.

## Where the schema lives

**Not here, and not in the application code.** `rust/migrations/` holds the
versioned DDL, applied by `uaas migrate`. These scripts create roles and
databases and nothing else.

That split matters. Everything in this directory runs only when the container's
data directory is empty, so it cannot be used to evolve a schema — a change
made here would never reach a database that already exists. Migrations can,
and they record what they have applied.
