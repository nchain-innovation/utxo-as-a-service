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

## The life of a spend

An outpoint is in exactly one of three places, and moves between them rather
than being flagged in place:

| Table | Meaning |
|---|---|
| `utxo` | spendable. Presence *is* spendability; there is no `spent` column |
| `utxo_spent`, `spent_height IS NULL` | a spend has been seen but not mined |
| `utxo_spent`, `spent_height` set | the spend is in a block at that height |

`utxo_unmined_spend` holds the outpoints in the middle state, with the chain
tip at which each spend was first seen. It duplicates something
`utxo_spent.spent_height IS NULL` already says, deliberately.

The obvious alternative — index `utxo_spent` to support that predicate —
cannot be used. Measured against `postgres:17` over 20 000 rows at
`fillfactor = 85`:

| Indexes on the table | HOT updates |
|---|---|
| none (control) | 16.0% |
| `btree (spent_seen_height)` | 16.0% |
| `btree (spent_seen_height) WHERE spent_height IS NULL` | **0.0%** |

A partial index counts the columns in its *predicate* as indexed. Naming
`spent_height` there means the settle's `UPDATE ... SET spent_height` modifies
an indexed column, and no update on `utxo_spent` can be heap-only again — which
is exactly what V6's `fillfactor = 85` exists to preserve, quietly undone. The
non-partial form keeps HOT but cannot separate unmined rows from the settled
ones sharing the column, so it degrades as the table grows.

The side table avoids the choice: `utxo_spent` gains no column, no index and no
lock, and the eviction scan runs over a table bounded by the mempool rather
than by the chain.

## Eviction

A spend that is broadcast and never mined would otherwise sit in `utxo_spent`
with a NULL `spent_height` for ever, leaving its outpoint in neither the
spendable set nor a settled spend — the UTXO set under-reported permanently,
with nothing logged. Two ordinary things cause it: a fee too low to be mined,
and the losing side of a double-spend.

After each block the service reclaims unmined spends older than
`[mempool] eviction_blocks` (default 144) and deletes the matching `mempool`
rows. Counts are logged at `warn`, not `info`, because `release_max_level_warn`
compiles `info!` out of release builds — a count logged at info would be
invisible in the deployment that needs it.

Age is measured in **blocks, not wall-clock time**. A timestamp keeps advancing
while the service is stopped or not syncing, so a restart after an outage would
reclaim everything at once; height only advances when the chain does.

Everything keys on the **outpoint**, never the spending txid, for the same
reason the settle does: a spend announced under one txid and mined as a
malleated sibling is the same spend.

## Conflicting spends

Two announcements can spend the same outpoint. The txid cannot tell you which
case you are looking at, because the txid covers the unlocking scripts and
those are malleable, so `spend_id::provisional_id` hashes only **what a spend
consumes and what it pays**:

| Prevouts | Outputs | Meaning |
|---|---|---|
| same | same | one spend, two encodings — malleation |
| same | different | two spends of one coin — a double spend |

Inputs are sorted into the hash and outputs are not: reordering inputs changes
nothing, while reordering outputs moves the coins, because an outpoint is a
txid and an index.

A refused spend does not add its outputs to the UTXO set. Without that, both
siblings' outputs were recorded as live and unrelated — 900 satoshis counted
twice from 1000 funded, with nothing logged. Malleation is reported at `warn`
and a genuine double spend at `error`, for the same reason the eviction counts
are at `warn`: `info!` is compiled out of release builds.

A refused transaction is still offered to the collections. A collection is a
record of transactions seen, not of the spendable set, and the conflict is
itself worth capturing.

Detection is on **positive evidence** — who already claims this outpoint —
never on the outpoint being absent from `utxo`, which since CS-421 is the
ordinary case for almost every transaction the service sees. There are two
sources of that evidence, both bounded:

* the unmined claims in `Utxo::spent_unmined`, bounded by the mempool;
* the outpoints spent earlier in the block being processed, bounded by the
  block.

An input with an all-zero txid claims nothing — that is the coinbase
convention, and no real output can have such a txid — so it is skipped.

### What this does not cover

Stated because the gaps are not obvious from the code:

* **A conflicting announcement arriving after the first spend was mined.** Once
  settled, the outpoint has no claim, and recognising it would need either a
  database lookup per input on the hot path or every spent outpoint held in
  memory, which grows with the chain rather than with the mempool.
* **The block does not win.** If A is seen unmined and its sibling B is then
  mined, B is refused because A already claims the outpoint. The *amount* is
  right — the coins are counted once — but they are recorded under A's txid,
  and A is the encoding that will never confirm. A query by B's txid finds
  nothing. First seen wins, where a block ought to.
* **Restarts.** Claims live in memory and are rebuilt only as new spends are
  seen, so a conflict spanning a restart is not detected.

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
