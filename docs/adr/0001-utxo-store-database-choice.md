# ADR-0001: Storage design for the UTXO indexing service

**Status:** **Accepted** 2026-09-06 — PostgreSQL, two-table split.
**Implementation:** `utxo-service-postgres-migration-plan.md`
**Date:** 2026-09-06
**Deciders:** UaaS maintainers
**Related:** `docs/postgres-storage-notes.md` (TOAST / MVCC / VACUUM / HOT background for the
storage reasoning below), `utxo-service-schema-init-plan.md`, `utxo-service-full-sweep.md`,
`utxo-service-review-findings.md`

---

## Revision note

The first draft of this ADR recommended putting the UTXO set behind a pluggable store trait
with an eye to an eventual key-value backend, and used the surrounding BSV node ecosystem as
supporting evidence.

**Both are withdrawn.** The ecosystem argument was weak — "another project chose this" is not
a reason, and it is not one the maintainers accept. And two requirements supplied since
invert the KV conclusion outright:

1. The service must answer **UTXO queries by block height**, both "the set as of height H"
   and "outputs created at height H".
2. The UTXO set is meant to be **restricted to monitored patterns** — with the important
   caveat that a pattern can be written to match every P2PKH output, so filtering is a
   policy, not a bound.

Requirement 1 is a range query over an interval. That is the thing a key-value store keyed on
outpoint is worst at, and the thing a relational engine is built for. The rest of this ADR is
rewritten around that.

---

## Context

### What the service is for

Not a general-purpose indexer. It watches for outputs matching operator-configured patterns
and maintains a queryable UTXO set for those. The UTXO set lives in a table because that made
it easy to query — which, given requirement 1, is the correct instinct.

### The scope question, and why filtering does not bound the size

The stated intent is monitored patterns only. The current code does not implement that:

```rust
// tx_analyser.rs::process_tx_outputs — no collection filter anywhere
for (index, vout) in tx.outputs.iter().enumerate() {
    if self.is_spendable(vout) {
        let pubkeyhash = script_to_pubkeyhash(&vout.lock_script);
        self.utxo.add(hash, index, vout.satoshis, height, &pubkeyhash);
    }
}
```

Every spendable output of every transaction is inserted, monitored or not. Closing that gap
is the single highest-leverage change available and it is cheap.

But it does not bound anything. `76a914[0-9a-f]{40}88ac` — already a commented-out example in
`data/uaasr.toml` — matches every P2PKH output on the chain. **The design must therefore work
well in the small case and degrade predictably, not catastrophically, in the degenerate one.**
That is a different requirement from "make it fast at full-node scale", and it is easier to
satisfy.

### What must be true, stated as queries

| # | Query | Frequency |
|---|---|---|
| Q1 | Is outpoint `(txid, vout)` currently unspent? | hot — every input of every transaction |
| Q2 | Current unspent outputs for identifier X | API |
| Q3 | Balance for identifier X | API |
| Q4 | **Outputs created at height H** | API — new |
| Q5 | **The unspent set as of height H** | API — new |
| Q6 | Unwind everything block H did | reorg |

Q5 is the one that decides the architecture. It is a predicate over a validity interval, and
answering it means knowing *when each output was spent*, not merely that it was — which the
current schema cannot represent, because a spend is a `DELETE`.

### The thing that has not changed

**Nobody has run this workload.** Since `89be757` (2024-11-19), no block
transaction has been written to the UTXO set and no spent output has been deleted — see
`utxo-service-review-findings.md` §0. Every performance question below is unmeasured, and
UAAS-01/02 must land before any of it can be measured. No claim in this ADR is backed by a
benchmark, and where I do arithmetic I say so.

---

## Decision

**One database. Relational. PostgreSQL. Temporal UTXO schema. No KV split.**

1. **Do not split storage.** The UTXO set stays in the same database as headers, block info,
   transactions and merkle-proof inputs.
2. **Move to PostgreSQL** — on the strength of two capabilities that directly serve Q1–Q5 and
   that MariaDB does not have (below), not on ecosystem grounds.
3. **Make the UTXO table temporal**: an output is a row with a validity interval
   `[created_height, spent_height)`. A spend is an `UPDATE`, not a `DELETE`.
4. **Filter inserts to monitored patterns**, and treat "a pattern matches everything" as a
   supported degenerate case with an explicit retention policy.

---

## Does the hybrid design make things incoherent?

Asked directly, so answered directly.

**Polyglot persistence is not incoherent as a pattern.** Bitcoin Core does exactly this —
LevelDB for chainstate, flat files for blocks, separate index databases — and it is the right
call there. The question is not whether the shape is respectable. It is whether it buys
anything *here*.

**Here, it does not, and it costs something specific.**

*What it would cost.* Applying a block must atomically insert the new outputs, mark the spent
ones, write the header, and write the transaction rows. In one database that is one
transaction. Across two stores there is no such thing, so you design a recovery protocol:
nominate one store as authoritative for progress, make the other idempotently replayable from
it, and handle the window where the process dies between the two commits. That is real design,
real testing, and a new class of failure — for a service whose entire state is reconstructible
from the chain anyway. Note the current code cannot even get this right *within* one database
(the writer's batches are not transactional at all — sweep §6.3); adding a second store before
fixing that is the wrong order.

*What it would buy.* At full-node scale, a lot: LSM trees turn random-key writes sequential
and compact deletes away as tombstones, and the binary encoding is roughly 5× smaller than the
current hex-in-varchar rows. That was the first draft's argument and it was sound **for a
full UTXO set**.

*Why it no longer applies.* Q5 is a range query over an interval. A KV store keyed on outpoint
answers it only by full scan, or by you hand-building and hand-maintaining secondary indexes
on `created_height` and `spent_height` — which is reimplementing the index machinery you
already have, without the transaction that keeps it consistent. Q4 is the same story. And the
scale that motivated the split is now operator-controlled rather than inherent.

So: coherent, but pointless here. **One store.**

---

## Why PostgreSQL specifically

Dropping the ecosystem argument leaves two capabilities that map onto the new requirements,
both verified rather than assumed.

### Partial indexes — MariaDB has none (weakened by the two-table split)

**Read this with the revision in "Liveness, confirmation and retention".** This was the
strongest single argument for Postgres when the design was one temporal table, because the
live-set index had to exclude history. The two-table split removes that need: `utxo` *is* the
live set, so a plain index is correct and no partial index is required.

Recording the capability difference anyway, because it still bears on the fallback and on any
future single-table variant. MariaDB 11.4 rejects the syntax outright:

```
CREATE INDEX idx_unspent ON utxo (pubkeyhash) WHERE spent_height IS NULL;
ERROR 1064 (42000): You have an error in your SQL syntax; ... near 'WHERE spent_height IS NULL'
-- MariaDB 11.4.13
```

Postgres and SQLite both support it. The practical consequence for **Option A** is that
MariaDB has no way to express a temporal single table efficiently — which is precisely why,
on MariaDB, the two-table split is not merely preferable but close to mandatory.

### BRIN indexes — Q4 and Q5 for kilobytes

Rows are inserted in block-height order, so `created_height` is naturally correlated with
physical order. That is exactly the case BRIN exists for: it stores min/max per block range
rather than an entry per row, making a height-range index orders of magnitude smaller than a
B-tree.

```sql
CREATE INDEX idx_utxo_created_brin ON utxo USING brin (created_height);
```

MariaDB has no equivalent. A B-tree on `created_height` works but costs full index size on the
largest table in the system.

### Also relevant, less decisive

- **Transactional DDL** — deletes §2 of `utxo-service-schema-init-plan.md` (the "one object
  per migration, no `IF NOT EXISTS`, half-applied migrations need manual repair" discipline)
  rather than working around it. That section exists only because MariaDB DDL is not
  transactional.
- **`BYTEA`** — 32 raw bytes per hash instead of 64 hex characters, byte-exact comparison, and
  it supersedes the `ascii_bin` workaround the schema plan needed to escape
  `utf8mb4_uca1400_ai_ci` comparing hashes case-insensitively.
- **`COPY`** for initial block download.
- **Range partitioning on `created_height`**, which gives partition pruning on Q5 and makes
  retention a `DETACH PARTITION` instead of a mass delete.

### What Postgres does not fix

**Vacuum.** I overstated this when I put the question to you: I said the temporal design makes
the delete-churn problem "disappear entirely". **It does not.** In MVCC an `UPDATE` is a new
tuple version plus a dead old one — the same cost as `DELETE`. The temporal design buys
correctness and queryability, not vacuum relief. (I also asserted here that `spent_height`
"has to be indexed", which turns out to be false and is now open question 2 — dropping that
index makes the settle HOT-eligible, and Q5 does not need it.) InnoDB's purge thread
has the same job and the same problem, so this is a wash between the engines rather than a
Postgres penalty — but it is not a win and should not be sold as one.

---

## Liveness, confirmation and retention

Revised again after maintainer feedback. The single-table temporal design below is
**withdrawn in favour of a two-table split**, for reasons I had underweighted.

### Your instinct is right, and here is the mechanism

"The UTXO table should only contain UTXO." Three arguments support that, and only the first
one is the aesthetic one:

1. **A table named `utxo` holding spent outputs is a naming lie**, and names are the cheapest
   documentation a schema has.
2. **Heap page density.** A partial index keeps the *index* small, but the heap it points into
   is the whole history. Q2/Q3 do index scan → random heap fetch, and those fetches land in a
   table where live rows are interleaved with history. A compact live table packs the rows you
   actually want onto far fewer pages. I claimed the partial index solved this; it solves half
   of it.
3. **Vacuum reaches a steady state instead of growing.** This is the real win and I got it
   backwards earlier. Under the single-table design a spend is an `UPDATE`, so the dead tuple
   lands in the big table and that table only ever grows — autovacuum rescans more every cycle,
   forever. Under the split a spend is `DELETE FROM utxo` + `INSERT INTO utxo_spent`: the dead
   tuple lands in a *small* table whose space is immediately reused by the next insert, so
   `utxo` stabilises at a size proportional to the live set. And `utxo_spent` is **append-only**
   — never updated, never deleted outside a reorg — which is the cheapest thing Postgres has to
   maintain.

The write cost is close to a wash, which is why (3) wins outright. The single-table `UPDATE`
writes a full new tuple regardless — and if `spent_height` is indexed it writes index entries
too — so it does much the same work as the delete-plus-insert, just deposited in the table you
least want it in. (Whether to index `spent_height` at all is open question 2; it does not
change this comparison, since the single-table design needs the dead tuple in the big table
either way.)

Worth noting: **the split is what partitioning would do anyway.** Range-partitioning on a
liveness expression would move rows between partitions on update, which Postgres implements as
delete-plus-insert internally. Writing it as two tables is the same mechanism, stated
explicitly, with less machinery.

### Mempool spends: move on sighting, settle the height on confirmation

Resolved, using the maintainer's proposal in preference to mine.

The service is a P2P listener, so it sees the spending transaction the moment it is broadcast.
At that point the output is **not usable** — a consumer building against it gets rejected by
the node. So the operationally correct answer is "not in the UTXO table", and the row should
move immediately:

| Event | Action |
|---|---|
| Spending tx seen on P2P | `utxo` → `utxo_spent`, `spent_txid` set, `spent_height` **NULL** |
| Block containing it arrives | `UPDATE utxo_spent SET spent_height = H` |
| Block spends an outpoint never seen in the mempool | `utxo` → `utxo_spent` with `spent_height = H` directly |

This is better than the nullable-marker design I proposed, for one reason that outweighs the
costs: **Q1 becomes a bare existence check.** "Is this spendable?" is `SELECT 1 FROM utxo
WHERE txid = $1 AND vout = $2` — no column test, no interpretation. Presence in `utxo` means
spendable, and that is the property the whole service exists to answer.

Two costs, both acceptable:

- **`utxo_spent` is no longer append-only**, which was one of the three reasons for splitting.
  It becomes *append-then-settle*: a row is inserted, then updated once, seconds to minutes
  later. Those rows are recent, hot in shared buffers, and physically clustered at the tail of
  the table, so the dead tuples are concentrated where vacuum reclaims them cheaply and the
  space is immediately reused. This is a well-behaved pattern, not the unbounded in-place
  churn the split was avoiding.
- **The confirmation path forks in two** — settle-in-place if the spend was already seen,
  move if it was not. Two set-based statements per block, no branching:

```sql
-- 1. settle spends already recorded from the mempool
UPDATE utxo_spent s SET spent_height = $H, spent_txid = i.spending_txid
FROM   unnest($txids, $vouts, $spending_txids) AS i(txid, vout, spending_txid)
WHERE  s.txid = i.txid AND s.vout = i.vout AND s.spent_height IS NULL;

-- 2. move anything still live (spend never seen in the mempool)
WITH moved AS (
    DELETE FROM utxo u
    USING unnest($txids, $vouts, $spending_txids) AS i(txid, vout, spending_txid)
    WHERE u.txid = i.txid AND u.vout = i.vout
    RETURNING u.*, i.spending_txid
)
INSERT INTO utxo_spent (txid, vout, satoshis, locking_script, identifier,
                        created_height, spent_txid, spent_height)
SELECT txid, vout, satoshis, locking_script, identifier,
       created_height, spending_txid, $H
FROM moved;
```

#### The one condition: key the settle on the outpoint, never on the spending txid

The obvious implementation of "update the entries with a block height" is
`WHERE spent_txid = <txid in block>`. **That is a trap**, and it breaks in a case this codebase
already has a review finding about.

Under Chronicle, a `v > 1` transaction has its malleability restrictions relaxed, so any
relaying node can rewrite its unlocking script and change its txid while every output byte
stays identical (findings §4). The service records `spent_txid = A` from the mempool; the
block confirms the same logical transaction as txid `B`; a settle keyed on txid matches
nothing. The row sits with `spent_height` NULL forever, recording a spend under a name that
was never mined, while the real spend goes unrecorded. Competing double-spends produce the
same failure.

**The outpoint is stable under malleation. The spending txid is not.** Keying on
`(txid, vout)` — as the statements above do — makes the settle self-correcting: it fixes
`spent_txid` to whatever the block actually contains. This costs nothing and must not be
skipped.

#### What still needs mempool eviction

If a broadcast spend never confirms, the row sits in `utxo_spent` with `spent_height` NULL and
the output is under-reported as unavailable. The failure direction is the safe one — a
consumer misses an output rather than building an invalid transaction — but it is still wrong,
and nothing clears it. Verified: the only path that removes a transaction from the mempool is
block inclusion (`txdb.rs:222`); there is no time- or size-based eviction anywhere in the
codebase.

The same mechanism covers the reorg case where a spend is unwound and never re-mined, so one
eviction implementation settles both. Pre-existing gap, own ticket.

### Retention, now answerable

Your backfill design settles it. The service starts at a chosen block height, and a separate
program walks the block files below that height looking for matching transactions. So:

- **Unspent outputs are never removed.** Never were in question.
- **Spent rows are kept back to the backfill start height** — because that is precisely the
  range over which you claim to answer Q5. Below it you have no data and above it you have all
  of it.

The retention horizon is therefore not a free parameter at all: **it equals the backfill
floor.** Nothing needs choosing. If you later backfill further back, the horizon moves with it.

### Confirmation stays derived

Unchanged, and it answers the question you asked directly: no, retention was not about
confirmation flagging. Confirmation depth is a subtraction against the tip
(`tip - created_height >= complete`, with `utxo.complete` currently 6), computed at query time.
Storing a `confirmed` flag would mean rewriting rows as the tip advances — write amplification
for arithmetic. `python/src/tx_analyser.py::get_balance` already derives it and should keep
doing so.

## The schema

```sql
-- Live set. Bounded by the live set, not by history.
CREATE TABLE utxo (
    txid           bytea    NOT NULL,
    vout           integer  NOT NULL,
    satoshis       bigint   NOT NULL,
    locking_script bytea    NOT NULL,     -- see the script-table note below
    identifier     bytea,                 -- from the pattern's named capture group
    created_height integer,               -- NULL = created by an unconfirmed tx
    PRIMARY KEY (txid, vout)
    -- No spent column. Presence in this table IS spendability.
);
CREATE INDEX idx_utxo_identifier  ON utxo (identifier);
CREATE INDEX idx_utxo_created_brn ON utxo USING brin (created_height);

-- Confirmed-spent history. Append-only except on reorg.
CREATE TABLE utxo_spent (
    txid           bytea    NOT NULL,
    vout           integer  NOT NULL,
    satoshis       bigint   NOT NULL,
    locking_script bytea    NOT NULL,
    identifier     bytea,
    created_height integer,               -- NULL if created by a tx still unconfirmed
    spent_txid     bytea    NOT NULL,     -- as last observed; corrected by the settle
    spent_height   integer,               -- NULL = spend broadcast, not yet mined
    PRIMARY KEY (txid, vout)
);
CREATE INDEX idx_spent_identifier ON utxo_spent (identifier);
CREATE INDEX idx_spent_created_br ON utxo_spent USING brin (created_height);
-- DELIBERATELY OMITTED, pending measurement: an index on spent_height.
-- Any index on that column disqualifies the settle UPDATE from HOT. See open question 2
-- and docs/postgres-storage-notes.md §5.

-- Which monitor(s) matched. Keyed on the outpoint, no FK: the outpoint lives in
-- whichever of the two tables currently holds it, and a FK cannot express that.
CREATE TABLE utxo_monitor (
    txid    bytea       NOT NULL,
    vout    integer     NOT NULL,
    monitor varchar(64) NOT NULL,
    PRIMARY KEY (txid, vout, monitor)
);
CREATE INDEX idx_utxo_monitor_name ON utxo_monitor (monitor);
```

Note the partial indexes are gone. They were compensating for history living in the live
table; with the split, `utxo` *is* the live set and a plain index is correct. **The Postgres
case now rests on BRIN, `BYTEA`, transactional DDL and `COPY`, not on partial indexes** — still
comfortably ahead of MariaDB, but the strongest single argument has been designed away rather
than won. Worth saying, since I led with it last time.

The queries:

```sql
-- Q1 currently unspent? — a bare existence check. Presence means spendable.
SELECT 1 FROM utxo WHERE txid = $1 AND vout = $2;

-- Q2/Q3 live outputs and balance for an identifier — touches only the small table
SELECT sum(satoshis) FROM utxo WHERE identifier = $1;

-- Q4 created at height H — both tables, since an output created at H may since be spent
SELECT txid, vout, satoshis, identifier FROM utxo       WHERE created_height = $1
UNION ALL
SELECT txid, vout, satoshis, identifier FROM utxo_spent WHERE created_height = $1;

-- Q5 the set as of height H
SELECT txid, vout, satoshis, identifier FROM utxo       WHERE created_height <= $1
UNION ALL
SELECT txid, vout, satoshis, identifier FROM utxo_spent
       WHERE created_height <= $1
         AND (spent_height IS NULL OR spent_height > $1);   -- NULL = still unmined, so live at H

-- NEW: in which block was this output spent?
SELECT spent_height, spent_txid FROM utxo_spent WHERE txid = $1 AND vout = $2;

-- Q6 unwind block H — idempotent, and it falls out of the same three states
UPDATE utxo_spent SET spent_height = NULL WHERE spent_height = $1;  -- spend is unmined again
DELETE FROM utxo_spent WHERE created_height = $1;                   -- created at H: gone
DELETE FROM utxo       WHERE created_height = $1;
```

Q5 becoming a `UNION ALL` is the price of the split, and it is a small one: two index scans
against tables that both prune on `created_height`. Q1/Q2/Q3 — the hot path — get strictly
better, hitting only the bounded table.

### The script-table question — measured, and it is engine-independent

Asked directly: is the row-move script copy a non-problem on Postgres and a problem on
MariaDB? **No — it is the same problem on both.** I checked rather than reasoned about it.

Postgres TOAST relations are **per-table**: `pg_toast.pg_toast_<oid>` belongs to one relation,
and a row in `utxo_spent` cannot reference a TOAST pointer owned by `utxo`. So an
`INSERT ... SELECT` between them detoasts and re-toasts — the bytes are physically copied.
Measured on PostgreSQL 17, 2000 rows of ~64 KB of incompressible script data:

```
--- after loading utxo only ---
  relname   |  heap   |   toast
------------+---------+------------
 utxo       | 136 kB  | 63 MB
 utxo_spent | 0 bytes | 8192 bytes

--- after INSERT INTO utxo_spent SELECT * FROM utxo; DELETE FROM utxo; ---
  relname   |  heap  | toast
------------+--------+-------
 utxo       | 136 kB | 63 MB      <- DELETE only marks dead; space returns on VACUUM
 utxo_spent | 136 kB | 63 MB      <- the full 63 MB was copied, not shared
```

MariaDB behaves the same way for the same reason: InnoDB stores a large `BLOB` in overflow
pages belonging to that table, with a 20-byte pointer in the row, and copying a row copies the
overflow pages. **The script-table decision is orthogonal to the engine choice.**

So the real question is whether a 2× lifetime write amplification on script bytes matters —
every output that is ever spent has its script written twice, once into `utxo` and once into
`utxo_spent`. A `script (script_hash, script)` table referenced by both makes it 1×, makes the
row move fixed-width and cheap regardless of script size, and dedupes identical scripts.

The wrinkle that stops this being sizeable up front: **the pattern mix is not fixed at deploy
time.** Operators add monitors at runtime through `POST /collection/monitor`, so a deployment
that starts out matching 25-byte P2PKH scripts can be pointed at megabyte data carriers
without a restart. A design sized for the former degrades quietly into the latter.

**Recommendation: start inline, and treat it as reversible.** Adding the indirection later is a
data migration, not a redesign, and there is no production data to make that awkward. But
instrument it — track the script-bytes-moved figure so the decision is made on a number rather
than on the pattern mix someone assumed at design time.

### What the backfill program means for this

The tool that walks block files below the start height is not a side concern — it writes half
the table. Four consequences:

1. **It must share the matcher with the live indexer**, not reimplement it. If the two disagree
   about what matches, the historical and live halves of the same table follow different rules
   and nothing downstream can tell. This is another reason for the matcher to become a library
   module (UAAS-07a) rather than staying inside `tx_analyser`.
2. **It should write each outpoint directly to its final table.** Backfilling a closed height
   range means the final state of every output in it is already known, so there is no reason to
   insert into `utxo` and then move — write live rows to `utxo` and already-spent rows straight
   to `utxo_spent`. The move path is only needed for live indexing.
3. **Bulk load, not row-at-a-time.** `COPY`, with the indexes created after the load.
4. **It must complete before the live indexer starts**, or the two race on the same outpoints.
   Sequence it as a prerequisite, the same way the migrate step is.

## Options considered

### Option A: Stay on MariaDB, two-table split

**Pros:** No engine switch. The two-table split, the temporal history and the Q4/Q6 wins are
all available on any SQL engine — none of them depend on Postgres. **The split having replaced
partial indexes as the mechanism for keeping the live set small, this option is now
substantially stronger than it was in the previous revision.**

**Cons:** No BRIN, so the `created_height` index on `utxo_spent` is a full B-tree on the
largest table in the system rather than a few kilobytes. Hashes stay hex-in-`varchar` unless
moved to `BINARY(32)`. Keeps the non-transactional DDL constraint, so the whole
one-object-per-migration discipline in the schema-init plan stays. Keeps the
case-insensitive-hash collation trap, worked around with `ascii_bin`.

**A genuinely viable fallback.** It delivers the correctness wins in full and costs more per
row and more migration discipline, forever.

### Option B: PostgreSQL, two-table split — *recommended*

**Pros:** Everything under "Why PostgreSQL". Q1–Q6 all served, Q6 correct by construction.

**Cons:** Rewrites both database layers (`mysql` → `postgres`; `mysql-connector-python` →
`psycopg`). `python/src/database.py` is 74 lines; the Rust side is larger but isolated behind
`database.rs`/`txdb.rs`/`utxo.rs`. `REPLACE INTO` → `INSERT ... ON CONFLICT`; `offset` is
reserved in both dialects. Postgres familiarity on the team is unknown. Vacuum is a standing
task and partitioning does not remove it.

### Option C: SQLite, two-table split

**Pros:** Transactional DDL — yes. Partial indexes — yes, though the split no longer needs
them. No server. One writer, many readers under WAL is this service's exact shape. Genuinely
strong on the merits, and the backfill program's bulk-load phase suits it well.

**Cons:** Not networked, so `python/src/*` loses direct database access and everything must
route through the Rust service over HTTP. No BRIN. Migrations hit `SQLITE_BUSY` if a reader
holds a transaction. Worth revisiting **only** if the Python service is folded into the Rust
one — at which point it becomes a serious contender.

### Option D: KV for the UTXO set, relational for the rest

**Withdrawn.** Q4 and Q5 make it the wrong structure; see the coherence section.

---

## Consequences

**Easier**

- Q4 and Q5 become ordinary queries instead of new machinery.
- The reorg unwind becomes two correct, idempotent statements, retiring the off-by-one and the
  tip-rollback bug (findings §4.3, ticket UAAS-09).
- The `height = -1` and `pubkeyhash = 'unknown'` sentinels both disappear, closing UAAS-14 and
  most of UAAS-19.
- `BYTEA` supersedes the `ascii_bin` workaround; migrations become atomic.
- `COPY` for IBD also retires the batch-truncation bug in sweep §6.3.
- Filtering to monitored patterns collapses the common case to a size where none of this is
  stressed.

**Harder**

- Two database layers rewritten and every statement re-dialected.
- The table never shrinks on its own; retention becomes a policy someone has to set.
- Vacuum tuning is a standing operational task, and partitioning does not remove it. In
  particular `autovacuum_vacuum_scale_factor` defaults to 0.2, so a large `utxo` would
  accumulate 20% dead tuples before autovacuum fires — it needs a per-table override from day
  one. See `docs/postgres-storage-notes.md` §4.
- Filtering means `/utxo/balance` answers only for monitored identifiers, and `calc_fee` loses
  prevout values for unmonitored inputs. It already returns 0 when any input is missing, so it
  degrades rather than breaks — but the API contract changes and should be documented.

**To revisit**

- After UAAS-01/02 land, measure IBD. Everything here is unmeasured.
- If the Python service is ever merged into the Rust one, reopen SQLite.
- If a match-everything pattern becomes a real deployment rather than a hypothetical, revisit
  partitioning granularity and the retention horizon with actual numbers.

---

## Action items

1. [ ] **Land UAAS-01 and UAAS-02.** Nothing here is measurable until the write path executes.
2. [ ] **Decide the engine** — Option B or the Option A fallback. Everything else is
       independent of that choice.
3. [ ] **Implement the monitored-pattern filter** in `process_tx_outputs`. Cheap, high
       leverage, and independent of the engine decision.
4. [ ] **Confirm Postgres familiarity on the team.** If thin, it is a real cost and Option A
       gets more attractive.
5. [ ] **Set the retention horizon** for as-of queries, and partition accordingly.
6. [ ] **Rewrite the schema-init plan's `V1..V8`** in the chosen dialect, against this schema.
       They are written, not ported — the plan's tables no longer match this design.
7. [ ] **Benchmark IBD** on the chosen engine with a match-everything pattern, to see what the
       degenerate case actually costs.
8. [ ] Update `docs/Database.md`, which documents a MySQL 8 setup for a MariaDB deployment.

---

## Resolved: the three open questions

### 1. Store the locking script — yes, and in the same table

Agreed, and for the reason you give: it is the thing driving selection, so not storing it
makes the index unable to explain its own decisions.

**Not a separate table.** In Postgres you would be hand-rolling something the engine already
does. Any `bytea` that would make a row exceed roughly 2 KB is compressed and, if still too
large, moved out of line into a per-table TOAST relation automatically — so large scripts
already live outside the main heap and do not bloat the pages that Q1/Q4/Q5 scan, while a
25-byte P2PKH script stays inline where it belongs. A manual side table would add a join to
every query that wants the script and duplicate that behaviour worse, because it would split
at a fixed boundary rather than at the size threshold.

Two things to do deliberately rather than by default:

- **Cap the script size on ingest.** Post-Genesis scripts are unbounded and attacker-chosen
  (findings §5). A cap belongs in config next to `max_broadcast_tx_bytes`, with oversized
  scripts truncated-and-flagged or the output skipped — your call which.
- Leave the column at the default `EXTENDED` storage (compress, then out-of-line). Scripts are
  repetitive and compress well; `EXTERNAL` would trade that away for substring-access speed
  the service does not need.

*Possible later optimisation, not now:* many outputs share an identical locking script — every
payment to one address has the same 25 bytes. A `script(script_hash, script)` side table would
dedupe them. For small scripts it is a net loss (a 32-byte hash key to avoid storing 25 bytes),
so it only pays if profiling shows large scripts dominating. Deliberately deferred.

### 2. Retention — resolved, and it needs no number

See "Retention, now answerable". Unspent rows are never removed. Spent rows are kept back to
the backfill start height, because that is exactly the range over which the service claims to
answer Q5. The horizon is not a free parameter — it equals the backfill floor and moves only
if you backfill further back.

### 3. Identifier NULL — agreed, but make it a declaration, not an accident

Your instinct is right, with one distinction: **silently storing NULL is correct when the
operator did not ask for an identifier, and a bug when they did.** Today the code cannot tell
those apart, because matching and extraction are unrelated — the pattern decides collection
membership, and `script_to_pubkeyhash` separately parses P2PKH by fixed byte offsets, returning
the string `'unknown'` for everything else. The `'unknown'` sentinel is that mismatch made
visible.

**Make extraction part of the pattern**, via a named capture group. Verified against the
pinned `regex` 1.12.3:

```
pattern 76a914(?<identifier>[0-9a-f]{40})88ac    groups=["identifier"] extracted=Some("7c78…29d9")
pattern 76a914(?P<identifier>[0-9a-f]{40})88ac   groups=["identifier"] extracted=Some("7c78…29d9")
pattern 76a914[0-9a-f]{40}88ac                   groups=[]             extracted=None
```

Both syntaxes work, and `Regex::capture_names()` tells you **at pattern-compile time** whether
the operator declared an identifier. That is what separates the two cases.

**The cases, documented as asked:**

| # | Case | `identifier` | Behaviour |
|---|---|---|---|
| 1 | Pattern declares no `identifier` group | `NULL` | Silent. Normal — the operator asked to match, not to extract. |
| 2 | Group declared and captured | bytes | Stored. |
| 3 | Group declared, pattern matched, group did not participate (e.g. it sits in an unmatched alternation branch) | `NULL` | **Log a warning.** The pattern is wrong, not the data. |
| 4 | Non-P2PKH match — `OP_RETURN` data protocols like the live `dsa` and `CoCv1` patterns | `NULL` | Case 1. There is no key to extract, and that is correct. |
| 5 | Match inside `OP_RETURN` trailing data (findings §1) | whatever was captured | Stored, **and it is meaningless** — the bytes are data, never executed. Not fixable by extraction; needs the structural matcher, UAAS-07b. |
| 6 | Nibble-misaligned match (sweep §2.2) | garbage | Disappears once matching moves to `regex::bytes` over `&[u8]` — UAAS-08. |
| 7 | P2PK, bare multisig, custom script | bytes, if the pattern declares them | Now expressible. The current fixed-offset P2PKH parse cannot represent these at all. |

Cases 5 and 6 are worth being explicit about: **they are not extraction bugs and no NULL policy
fixes them.** A confidently-extracted identifier from `OP_RETURN` data is exactly the injection
channel in findings §1. Extraction reports what the pattern found; it does not establish that
the bytes mean anything.

**On re-parsing.** You asked whether the transaction could be re-parsed later, or re-requested
from the node. Neither is needed, and this is where your two answers combine well: **because
the locking script is stored, `identifier` becomes a derived, backfillable column.** If an
extraction rule changes or a pattern is corrected, recompute it in place —

```sql
UPDATE utxo SET identifier = ... WHERE identifier IS NULL AND <pattern matches>;
```

— with no node round-trip and no dependence on `save_txs`, which is `false` by default so the
transaction may not be in the table at all. Re-requesting from the node would put a network
dependency in a query path and can fail; storing the script makes re-extraction a local
operation. That is a good reason to store it beyond the one you gave.

## Remaining open questions

1. **Script storage.** Resolved as "start inline, revisit on measurement" — see the
   script-table section. Not an engine question: both Postgres and MariaDB physically copy the
   script on the row move (measured). Needs instrumentation, not a decision, right now.
2. **Mempool eviction.** The provisional `spent_txid` marker needs something to clear it when a
   broadcast spend never confirms. No eviction of any kind exists today (verified: the only
   removal path is block inclusion, `txdb.rs:222`), so this is a pre-existing gap the split
   makes visible rather than one it creates. Needs its own ticket.
3. ~~**Index `utxo_spent.spent_height`, or not?**~~ **RESOLVED 2026-09-06 by measurement.**
   Use a **BRIN** index, and set `fillfactor = 85` on the table. PostgreSQL 16 stopped
   counting summarizing indexes against HOT eligibility, so BRIN measures identically to
   having no index at all (25% HOT in both cases) while a btree takes it to 0%. The trade
   below does not exist on PG 16+. Separately, the default `fillfactor = 100` makes HOT
   impossible regardless of indexing — that is what actually needed fixing. Detail and figures
   in `utxo-service-postgres-migration-plan.md` §11.1. Original reasoning retained below for
   the record:

   ~~The question as originally posed:~~ A HOT (Heap-Only Tuple) update — one that
   changes no indexed column and fits on the same page — writes the new row version without
   touching any index, and the dead version is reclaimable by page pruning rather than a full
   vacuum. The rule is strict: *any* index on a modified column disqualifies it.

   The settle step updates `spent_height`, so this is a straight trade:

   | | Reorg unwind `WHERE spent_height = H` | Settle `UPDATE … SET spent_height` |
   |---|---|---|
   | **Indexed** | index seek | never HOT — index writes on every settle |
   | **Not indexed** | scan | HOT-eligible — no index writes, cheap pruning |

   Settles happen once per spend, forever. Reorgs are rare and touch only recent heights, which
   sit at the physical tail of the table and are therefore cheap to scan. **That argues for
   omitting the index**, and the schema above omits it — but it is measurable rather than
   arguable and should be decided on numbers from the IBD benchmark, not on this reasoning.

   Note also that Q5 does not need it: the selective predicate there is `created_height`,
   served by BRIN, with `spent_height` applied as a filter. Background in
   `docs/postgres-storage-notes.md` §5.

4. ~~**Engine.**~~ **Decided: PostgreSQL** (Option B), 2026-09-06. Recorded for the file that
   the case shifted during the discussion — partial indexes were the strongest single argument
   and the two-table split designed them away, so the accepted decision rests on BRIN,
   `BYTEA`, transactional DDL and `COPY`, plus the migration ergonomics.
