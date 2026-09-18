# Rebuilding the UTXO set (reindex)

## Why this is needed

Commit `89be757` (2024-11-19) converted two `for` loops in
`rust/src/uaas/tx_analyser.rs` into lazy `Iterator::map` adapters bound with
`let _ =`. `Iterator::map` is lazy — it builds an adapter and does nothing
until something drives it — so neither closure ran again:

- `process_tx_inputs` never called `Utxo::delete`, so **no spent output was
  ever removed** from the UTXO set, in a block or in the mempool.
- `process_block` never called `process_block_tx`, so **no transaction in any
  block was ever added** to the UTXO set.

The only surviving writer was the mempool path
(`process_standalone_tx` → `process_tx_outputs`), which records
`height = NOT_IN_BLOCK` (`-1`).

The consequence for any database written by a build between 2024-11-19 and
this fix:

| Table | State |
|---|---|
| `utxo` | Append-only. Every row has `height = -1`. No row has ever been deleted. Contains only outputs seen in the mempool, and contains them whether or not they were later spent. |
| `blocks` | Correct — the block-header path was never affected. |
| `tx` | Correct when `save_txs = true`. |
| `mempool` | Correct — entries are removed when a block confirms them. |

`utxo` cannot be repaired in place. There is no record of which outputs were
spent, and the confirmed set was never written at all. It has to be rebuilt.

## What the fix changes at runtime

Restoring eager iteration is not a no-op for resource use. Once these loops
run:

- **The in-memory UTXO map holds the full spendable UTXO set**, not just
  mempool outputs. `process_tx_outputs` filters only on `is_spendable`, not on
  the configured collections, so every spendable output of every block is
  retained. On mainnet this is a multi-gigabyte structure. Size the host
  before starting a mainnet reindex.
- **Database write volume rises by orders of magnitude.** Every block now
  produces a `UtxoBatchWrite` and a `UtxoBatchDelete`.
- **`calc_fee` starts returning real fees.** It walks `tx.inputs` and returns
  `0` if any input is missing from the UTXO set. With the confirmed set absent,
  it returned `0` for nearly every transaction. Fees recorded in `mempool.fee`
  after this fix will differ from those recorded before it. This is a change in
  a computed value, not a refactor.

## Procedure

> Not yet executed end to end. The steps below are derived from reading
> `block_manager.rs` and `database.rs`, not from a completed reindex run.
> Treat the timings as unknown.

### 1. Stop the service

```bash
docker compose stop uaas_backend uaas_web
```

### 2. Truncate the affected tables

Dialect: **PostgreSQL 17**. Unlike MariaDB, where `TRUNCATE` is DDL that
commits implicitly and cannot be rolled back, PostgreSQL's DDL is transactional
— wrap these in `BEGIN`/`COMMIT` and a mistake is recoverable with `ROLLBACK`
until you commit. Take a dump anyway if the `blocks` table is expensive to
rebuild.

```sql
BEGIN;
TRUNCATE TABLE utxo;
TRUNCATE TABLE mempool;
TRUNCATE TABLE blocks;
TRUNCATE TABLE tx;      -- only if save_txs = true
COMMIT;
```

`TRUNCATE` takes an `ACCESS EXCLUSIVE` lock on each table, so nothing can read
them while the transaction is open. That is why the service is stopped first
rather than relying on the lock to serialise against it.

Truncating `blocks` and `tx` is now a choice rather than a requirement. Under
MariaDB the writer used a plain `INSERT` against the primary key on `hash`, so
replaying a block into a populated table failed on a duplicate key. Every
insert on this path is now `ON CONFLICT (hash) DO NOTHING`
(`database.rs:322`, `:429`), and `utxo` is `ON CONFLICT (txid, vout) DO UPDATE`
(`database.rs:267`), so a replay is idempotent. Truncate them to rebuild from
nothing; leave them to replay over what is there.

Leave `collection` alone unless the collection definitions have changed; it is
keyed independently of block height.

### 3. Choose a source of blocks

`BlockManager::setup` branches on `startup_load_from_database`:

- `true` — headers are read from the `blocks` table and **no transaction is
  reprocessed**. This is the normal restart path and will not rebuild `utxo`.
- `false` — `read_blocks_from_file` replays `block_file` through
  `process_block`, which now populates the UTXO set.

**Offline reindex (preferred).** If the deployment has been running with
`save_blocks = true`, the local block file already holds the chain from
`start_block_height`. Set in `data/uaasr.toml` for the active network:

```toml
startup_load_from_database = false
block_file = "../data/main-block.dat"   # or the testnet file
```

No peer connection is needed to rebuild from the file, so this route does not
touch a node.

**Network reindex.** With no block file, the service re-downloads from
`start_block_height` over P2P. Confirm the target node before starting; do not
point a reindex at a mainnet node without clearing it first.

### 4. Restart and watch

```bash
docker compose up -d uaas_backend
docker compose logs -f uaas_backend
```

`read_blocks_from_file` logs `process_block = <hash> <timestamp>` per block and
a `blocks read in N seconds` summary at the end.

Out-of-order blocks are logged as `Skipping out-of-order block ...` and are
**not** retried from the file path — `process_block` returns early when
`prev_hash` does not match `last_hash_processed`. If that line appears, the
resulting UTXO set is incomplete; stop and investigate rather than letting the
run finish.

### 5. Verify

```sql
-- No row should still carry the sentinel height once the chain is caught up,
-- except genuine mempool entries.
SELECT height, COUNT(*) FROM utxo GROUP BY height ORDER BY height LIMIT 10;

-- Highest confirmed height in the UTXO set should track the block tip.
SELECT MAX(height) FROM utxo;
SELECT MAX(height) FROM blocks;
```

Uses `idx_utxo_height` for the `GROUP BY` and the `MAX` — that index is created
by `schema::ensure_performance_indexes`. This is read from the DDL, not from an
`EXPLAIN` on a populated table.

## Read-API behaviour during a reindex

`GET /utxo/balance` and the other UTXO endpoints read the same tables that are
being rebuilt. During a reindex they return a partial set: balances climb from
zero towards the true figure as blocks are replayed. There is no readiness gate
on the UTXO endpoints specifically — `/health` reports only database
reachability, and the `ready` state in `logic.rs` tracks block sync, not UTXO
completeness.

Either take the service out of rotation for the duration, or accept that
balance queries are wrong until the run completes. Do not leave a partially
reindexed instance serving traffic that is trusted for value decisions.
