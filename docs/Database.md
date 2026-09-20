# Database

**PostgreSQL 17.** Both the Rust indexer and the Python REST API read and write
the same database, through the same connection URL in `[database]`.

## Schema and migrations

The schema lives in `rust/migrations/`, as `V{n}__{name}.sql`, one object per
file. It is **not** created by the application and **not** in
`init_database/`, which is a change from how this service has always worked.

### Why not in the application

Until this change, all eight tables and six indexes were created by Rust at
startup, each guarded by a query against `INFORMATION_SCHEMA` followed by a
bare `CREATE TABLE`. Three problems, none of which is fixable while the
schema lives in code:

* the check and the create are separate statements, so two instances starting
  together race;
* the existence probe does not filter by schema, so a same-named table anywhere
  visible suppresses creation;
* each core table had three or four definitions — production code, test code,
  CI heredocs — which had already drifted from one another.

### Why not in `init_database/`

Anything mounted at `/docker-entrypoint-initdb.d/` runs **only when the
container's data directory is empty**. It cannot evolve a database that already
exists, so it can create a schema once and never change it. That directory now
holds roles and databases only.

### Applying them

```bash
uaas migrate postgresql://uaas:uaas-password@localhost:5433/uaas_db
```

Or set `UAAS_POSTGRES_URL` and run `uaas migrate`. Safe to repeat: applied
migrations are skipped. The files are embedded in the binary at compile time,
so the published image carries no `psql` and no `.sql` files.

### What it refuses to do

| Situation | Behaviour |
|---|---|
| A migration file edited after it was applied | Refused, naming the file. An applied migration is history; add a new one instead. |
| The database records a migration this build does not have | Refused. The binary has been rolled back without its schema. |
| The service starts against a schema at the wrong version | Refused at startup, naming both versions, rather than failing later at a query. |

Each migration and its bookkeeping row commit in one transaction. PostgreSQL's
DDL is transactional, so a migration that fails part-way leaves nothing behind
— no half-created table and no row claiming success. This is the reason the
runner is ~150 lines rather than a dependency: the manual-repair procedure a
MySQL-family runner needs does not exist here, because MySQL-family DDL commits
implicitly.

### Adding a migration

1. Add `rust/migrations/V{n}__{name}.sql` with the next number.
2. Register it in `MIGRATIONS` in `rust/src/migrate.rs` and bump
   `EXPECTED_VERSION`.
3. `cargo test --lib migrate` — `mig01` fails if the file and the list disagree.

Never edit a file that has already been applied anywhere.

## Connecting by hand

`docker compose up` brings up PostgreSQL on host port **5433**, with the roles
and databases from `init_database/postgres/01_roles_and_databases.sql` and the
schema applied by the one-shot `uaas_migrate` service.

```bash
psql postgresql://uaas:uaas-password@localhost:5433/uaas_db
```

Adminer, at <http://localhost:8080>, is a GUI over the same database. Choose
"PostgreSQL" as the system and `postgres` as the server — the compose service
name, not `localhost`, because Adminer connects from inside the compose network.

## Byte order

Hashes are stored as 32 raw bytes in `bytea`, in **internal** order — the
reverse of the order a txid is conventionally displayed and the order both APIs
speak. `Hash256.0` on the Rust side and `hashes.py` on the Python side do the
conversion; `rust/tests/hash_order.rs` and `python/tests/test_hashes.py` pin the
two against the same literals.

This matters because getting it wrong is silent. The wrong bytes compared to a
`bytea` column match no rows and raise nothing.

Identifiers are **not** reversed. An identifier is the bytes a locking-script
pattern captured, taken from the script as written.
