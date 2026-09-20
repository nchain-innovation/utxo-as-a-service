# PostgreSQL storage notes: TOAST, MVCC, VACUUM and HOT

**Audience:** anyone working on the UaaS schema.
**Scope:** the four Postgres storage mechanisms that actually shape decisions in
`docs/adr/0001-utxo-store-database-choice.md`. Not a general Postgres tutorial — every
section ends with what it means for the `utxo` / `utxo_spent` design.
**Version:** PostgreSQL 17. Constants and quotes are from the official documentation, cited
inline. Measurements were run locally against `postgres:17` and are reproducible from the
appendix.

Where something was measured rather than read, it says so. Where a measurement failed, it
says that too.

---

## 1. TOAST

### The problem

Postgres stores rows in fixed **8 kB pages**, and a row cannot span pages. Without help, a
1 MB locking script would simply be unstorable.

**TOAST** — The Oversized-Attribute Storage Technique — is the answer. When a row would
exceed `TOAST_TUPLE_THRESHOLD`, "normally 2 kB" ([storage-toast]), Postgres shrinks it in
order:

1. **Compress** the largest variable-length attributes, if the column's storage strategy
   allows it.
2. If the row is still too large, **move the largest attributes out of line**, leaving a
   pointer. "The total size of an on-disk TOAST pointer datum is therefore 18 bytes regardless
   of the actual size of the represented value."
3. Repeat until the row fits.

### The mechanism

"If any of the columns of a table are TOAST-able, the table will have an associated TOAST
table." Out-of-line values are split into chunks — "at most `TOAST_MAX_CHUNK_SIZE` bytes (by
default this value is chosen so that four chunk rows will fit on a page, making it about 2000
bytes)" — and stored as rows in it.

```
  main table: utxo                      TOAST table: pg_toast.pg_toast_<oid>
  ┌──────┬──────┬──────────────────┐    ┌──────────┬───────────┬────────────┐
  │ txid │ sats │ locking_script   │    │ chunk_id │ chunk_seq │ chunk_data │
  ├──────┼──────┼──────────────────┤    ├──────────┼───────────┼────────────┤
  │ ab12 │ 5000 │ [18-byte ptr]────┼───▶│   4711   │     0     │ ~2000 B    │
  │ cd34 │ 1200 │ 76a914…88ac      │    │   4711   │     1     │ ~2000 B    │
  └──────┴──────┴──────────────────┘    │   4711   │     2     │ ~2000 B    │
         ▲                              └──────────┴───────────┴────────────┘
         └─ 25-byte P2PKH script          + a btree index on (chunk_id, chunk_seq)
            stays inline
```

### The property that drives our schema

**A TOAST table belongs to exactly one parent table.** `chunk_id` values are allocated from
that specific TOAST relation, so a pointer written in a `utxo` row is meaningless in
`utxo_spent`. There is no mechanism to share chunks between tables.

MariaDB/InnoDB behaves the same way for the same reason: a large `BLOB` lives in overflow
pages owned by one table, referenced by a 20-byte pointer in the row. **Neither engine can
move a row between tables without copying its large values.**

### De-TOASTing and re-TOASTing

**De-TOASTing** is reading a value back: follow the pointer, fetch chunks by `chunk_id` from
the TOAST index, concatenate, decompress. It is **lazy** — a query that never references the
column never pays for it. `SELECT count(*)` over a table of megabyte scripts is fast, and
Q1/Q2/Q3 in the ADR never touch the script column at all.

**Re-TOASTing** is writing a value somewhere that needs a new pointer: a fresh `chunk_id` in
the target's TOAST relation, chunks written out again.

**Measured** — moving 2000 rows carrying ~64 kB of incompressible script data each:

```
--- after loading utxo only ---
  relname   |  heap   |   toast
 utxo       | 136 kB  | 63 MB
 utxo_spent | 0 bytes | 8192 bytes

--- after INSERT INTO utxo_spent SELECT * FROM utxo; DELETE FROM utxo; ---
  relname   |  heap  | toast
 utxo       | 136 kB | 63 MB      <- DELETE only marks dead; see §2
 utxo_spent | 136 kB | 63 MB      <- the full 63 MB was copied, not shared
```

### The exception that matters

An `UPDATE` that leaves the toasted column alone does **not** re-TOAST. From the
documentation: *"During an `UPDATE` operation, values of unchanged fields are normally
preserved as-is; so an `UPDATE` of a row with out-of-line values incurs no TOAST costs if none
of the out-of-line values change."*

**Measured**, updating only a fixed-width column:

```
--- baseline ---                              heap: 136 kB   toast: 63 MB
--- after UPDATE SET spent_height = 776082 -- heap: 288 kB   toast: 63 MB
```

The heap doubled (new tuple versions, §2) and the TOAST relation did not move.

### Storage strategies

Set per column with `ALTER TABLE … ALTER COLUMN … SET STORAGE`:

| Strategy | Compression | Out-of-line | Notes |
|---|---|---|---|
| `PLAIN` | no | no | Only option for non-TOAST-able types. Fails if too big. |
| `EXTENDED` | yes | yes | **Default for `bytea`.** Compress first, then move out of line. |
| `EXTERNAL` | no | yes | "Will make substring operations on wide `text` and `bytea` columns faster (at the penalty of increased storage space)." |
| `MAIN` | yes | last resort | Compress, keep inline unless there is no other way. |

**For `locking_script`: leave it at `EXTENDED`.** Scripts are repetitive and compress well, and
nothing in this service substrings them, so `EXTERNAL` would trade space for speed we do not
use.

### What it means for our design

- **Q1/Q2/Q3 do not pay for the script column.** De-TOASTing is lazy.
- **Moving a row between `utxo` and `utxo_spent` copies its script.** Every output that is
  ever spent has its script written twice over its lifetime — 2× write amplification on script
  bytes. This is engine-independent.
- **The settle `UPDATE` is cheap regardless of script size**, documented and measured. Only the
  move pays.
- A shared `script (script_hash, script)` table would make the move fixed-width and reduce the
  amplification to 1×, at the cost of a join. Deferred as a measurement, not a guess — see the
  ADR.

---

## 2. MVCC — why dead rows exist

**Postgres never updates a row in place.** An `UPDATE` writes a new tuple and marks the old one
dead. A `DELETE` only marks dead. Each tuple carries `xmin` (creating transaction) and `xmax`
(deleting transaction); a transaction sees a tuple when `xmin` is committed and visible to it
and `xmax` is not.

```
  T100 INSERT       T140 UPDATE        T180 DELETE
  ┌──────────────┐  ┌──────────────┐   (no new version;
  │ xmin=100     │  │ xmin=140     │    xmax set on the
  │ xmax=140  ✗  │  │ xmax=180  ✗  │    live tuple)
  └──────────────┘  └──────────────┘
       dead              dead
```

This is how readers never block writers: a transaction that started at T150 still sees the
middle version while T180 deletes it. The cost is that **dead tuples accumulate** — "bloat".

If it helps to anchor it: this is closer to a garbage-collected arena than to in-place
mutation. Nothing is freed at the moment of the write; a collector reclaims it once no one can
still be looking.

---

## 3. VACUUM

Vacuum finds tuples dead to **every** currently running transaction and:

1. Marks their space reusable **within that table**, via the free space map.
2. Removes the matching index entries.
3. Updates the **visibility map** — pages where all tuples are visible to everyone — which is
   what allows an index-only scan to skip the heap fetch. A well-vacuumed table gives
   measurably faster index-only reads.
4. **Freezes** old tuples (below).

### What it does not do

It does not return space to the operating system. From the documentation: *"The standard form
of `VACUUM` removes dead row versions in tables and indexes and marks the space available for
future reuse. However, it will not return the space to the operating system, except in the
special case where one or more pages at the end of a table become entirely free and an
exclusive table lock can be easily obtained. In contrast, `VACUUM FULL` actively compacts
tables by writing a complete new version of the table file with no dead space."*

That is why the measurement in §1 still showed `utxo` at 63 MB after every row was deleted.

**For a steady-state table this is exactly what you want** — delete, then the next insert
reuses the hole and the file stops growing. `VACUUM FULL` takes an `ACCESS EXCLUSIVE` lock and
blocks all access, so it is a maintenance-window tool, never an operational one.

### Freezing and transaction ID wraparound

`xmin`/`xmax` are 32-bit. After enough transactions the counter wraps, and an ancient tuple
would appear to be from the future — and become invisible. Postgres avoids this by freezing:
*"PostgreSQL reserves a special XID, `FrozenTransactionId`, which does not follow the normal
XID comparison rules and is always considered older than every normal XID."*

The requirement is absolute: *"it is necessary to vacuum every table in every database at least
once every two billion transactions."* If freezing falls too far behind, Postgres refuses
writes to protect the data. `autovacuum_freeze_max_age` defaults to *"200 million
transactions"*.

**So even a purely append-only table must be vacuumed periodically.** The ADR calls
`utxo_spent` cheap to maintain — cheap, but never free.

---

## 4. autovacuum

A launcher plus worker processes (`autovacuum_max_workers`, default **three**) that wake every
`autovacuum_naptime` (default **1 min**) and vacuum tables whose thresholds are crossed.

### The trigger

> vacuum threshold = vacuum base threshold + vacuum scale factor × number of tuples

with `autovacuum_vacuum_threshold` default **50 tuples** and
`autovacuum_vacuum_scale_factor` default **0.2 (20% of table size)**.

**That 20% is the single biggest operational trap on a large table.** A billion-row table
accumulates 200 million dead tuples before autovacuum starts. By then the vacuum is enormous,
runs long, and more dead tuples pile up while it runs — bloat makes vacuum slower, slow vacuum
allows more bloat. Override it per table:

```sql
ALTER TABLE utxo SET (autovacuum_vacuum_scale_factor = 0.02,
                      autovacuum_vacuum_threshold    = 10000);
```

### Insert-triggered vacuum

Since PG 13 there is a second trigger, for tables that only ever grow:

> vacuum insert threshold = vacuum base insert threshold + vacuum insert scale factor × number of tuples

with `autovacuum_vacuum_insert_threshold` default **1000 tuples** and
`autovacuum_vacuum_insert_scale_factor` default **0.2**. This exists so append-only tables get
visited for freezing and visibility-map maintenance without ever producing a dead tuple —
directly relevant to `utxo_spent`.

### Throttling

`autovacuum_vacuum_cost_delay` (default **2 ms**) and `autovacuum_vacuum_cost_limit` (default
**-1**, meaning inherit `vacuum_cost_limit`) deliberately slow vacuum's I/O so it does not
swamp foreground work. Conservative by default, and a common reason vacuum cannot keep up on a
write-heavy table.

---

## 5. HOT — Heap-Only Tuples

If an `UPDATE` changes **no indexed column** and there is room on the same page, Postgres
writes the new version on that page and chains it from the old, **touching no index at all**.
The dead version can then be reclaimed by page pruning without waiting for a full vacuum.

Two conditions, and both matter:

1. The update changes no column carried by a **non-summarizing** index. Since PostgreSQL 16,
   **BRIN indexes do not count** — a summarizing index stores min/max per block range rather
   than a pointer per row, so nothing needs updating when a row moves.
2. There is free space on the page. Tables default to `fillfactor = 100`, meaning pages are
   packed full at insert — so a freshly loaded table has **no room for HOT at all**, whatever
   its indexes look like.

### Measured

Updating `spent_height` across 50,000 rows on PostgreSQL 17, with a btree index present on
another column (`identifier`):

```
fillfactor=80, updating spent_height on 50k rows:
no index on spent_height   ->  updates=50000  hot=12504  (25% HOT)
BRIN  on spent_height      ->  updates=50000  hot=12504  (25% HOT)
btree on spent_height      ->  updates=50000  hot=0      ( 0% HOT)
```

**BRIN is indistinguishable from no index.** A btree on the updated column takes HOT to zero.

The first run of this test used the default `fillfactor = 100` and returned **0% HOT in every
case, including with no index at all** — which nearly produced the wrong conclusion. Condition
2 above was the cause. Any HOT measurement on a freshly loaded table is meaningless unless
fillfactor was lowered first.

Reading the counters needs care too: `pg_stat_user_tables` is fed by an asynchronous
statistics collector, so an immediate query returns stale values. Call
`pg_stat_force_next_flush()` from a separate transaction, or sleep, before reading.

### The design consequence

`utxo_spent` is inserted once and updated exactly once (the settle). So it gets **BRIN on
`spent_height`** — reorg-unwind pruning at no cost to HOT — and **`WITH (fillfactor = 85)`**
to leave room for the settle's new row version on the same page. Both are in the migration
plan §3; the reasoning is §11.1 there.

---

## Appendix: reproducing the measurements

```bash
docker run -d --name pg_probe -e POSTGRES_PASSWORD=probe postgres:17
# wait for pg_isready, then:
docker exec -i pg_probe psql -U postgres
```

```sql
CREATE TABLE utxo       (txid bytea PRIMARY KEY, spent_height int, locking_script bytea);
CREATE TABLE utxo_spent (txid bytea PRIMARY KEY, spent_height int, locking_script bytea);

-- 2000 rows x ~64 kB of incompressible data
INSERT INTO utxo
SELECT decode(md5(g::text),'hex'), NULL,
       decode(string_agg(md5(random()::text),''),'hex')
FROM generate_series(1,2000) g, generate_series(1,2000) h
GROUP BY g;

CREATE OR REPLACE VIEW sizes AS
SELECT c.relname,
       pg_size_pretty(pg_relation_size(c.oid))       AS heap,
       pg_size_pretty(pg_total_relation_size(t.oid)) AS toast
FROM pg_class c LEFT JOIN pg_class t ON t.oid = c.reltoastrelid
WHERE c.relname IN ('utxo','utxo_spent') ORDER BY 1;

SELECT * FROM sizes;                                    -- baseline

-- (a) does moving a row copy its TOAST data?
INSERT INTO utxo_spent SELECT * FROM utxo;
DELETE FROM utxo;
SELECT * FROM sizes;                                    -- utxo_spent toast: 8 kB -> 63 MB

-- (b) does updating a non-toasted column rewrite TOAST?
UPDATE utxo_spent SET spent_height = 776082;
SELECT * FROM sizes;                                    -- heap grows, toast unchanged
```

```bash
docker rm -f pg_probe
```

---

## Sources

- [storage-toast] TOAST — https://www.postgresql.org/docs/17/storage-toast.html
- Routine vacuuming — https://www.postgresql.org/docs/17/routine-vacuuming.html
- Autovacuum configuration — https://www.postgresql.org/docs/17/runtime-config-autovacuum.html
- Heap-Only Tuples — https://www.postgresql.org/docs/17/storage-hot.html
