//! The database writer thread.
//!
//! Every write the indexer performs arrives here on a channel, so the peer
//! threads are never blocked on the database during initial block download.
//!
//! # Signed columns
//!
//! PostgreSQL has no unsigned integer types, so every `u32`/`u64` that used to
//! go into an `int unsigned` or `bigint unsigned` column now has to fit a
//! signed one. These values come off the P2P wire and are attacker-chosen, so
//! each conversion is checked explicitly rather than cast. A value that does
//! not fit is dropped with an error naming it — writing a wrapped negative
//! would corrupt the row silently, which is worse than losing it loudly.
//!
//! # Batches are transactional
//!
//! Each batch is applied inside one transaction, so a batch that fails part way
//! leaves nothing behind. Previously `exec_batch` applied rows one at a time
//! with no transaction, and a failure in the middle left the earlier rows
//! written and the later ones not, with no record of where it stopped.

use std::sync::mpsc;
use std::time::{Duration, UNIX_EPOCH};

use chain_gang::{messages::OutPoint, util::Hash256};

use postgres::types::ToSql;

use crate::{config::Config, db::PooledConn};
use retry::{delay, retry};

// UtxoEntry - used to store data into utxo table
#[derive(Clone)]
pub struct UtxoEntryDB {
    /// Raw 32-byte txid. Was a 64-character hex string in a `varchar(64)`.
    pub hash: Vec<u8>,
    pub pos: u32,
    pub satoshis: i64,
    /// `NOT_IN_BLOCK` (-1) for an output not yet in a block, which is written
    /// as SQL NULL — see [`height_to_sql`].
    pub height: i32,
    /// The locking script the output pays to.
    ///
    /// Carried in full now. It was dropped from the in-memory entry because
    /// some scripts are very large, and written as an empty placeholder — which
    /// the settle would then copy into `utxo_spent`, so both tables would have
    /// lied. Recording only monitored outputs is what makes keeping it
    /// affordable: the volume is what the patterns select, not every spendable
    /// output on the chain.
    pub locking_script: Vec<u8>,
    /// The bytes the matching pattern captured in its `identifier` group.
    pub identifier: Option<Vec<u8>>,
}

// Used to store txs to write (in blocks)
#[derive(Clone)]
pub struct TxEntryWriteDB {
    pub hash: Hash256,
    pub height: usize,
    pub blockindex: u32,
    pub size: u32,
    pub satoshis: u64,
}

// database header structure
#[derive(Clone)]
pub struct BlockHeaderWriteDB {
    pub height: u32,
    pub hash: Hash256,
    pub version: u32,
    pub prev_hash: Hash256,
    pub merkle_root: Hash256,
    pub timestamp: u32,
    pub bits: u32,
    pub nonce: u32,
    pub position: u64,
    pub blocksize: u32,
    pub numtxs: u32,
}

#[derive(Clone, Default)]
pub struct OrphanBlockHeaderWriteDB {
    pub height: u32,
    pub hash: Hash256,
    pub version: u32,
    pub prev_hash: Hash256,
    pub merkle_root: Hash256,
    pub timestamp: u32,
    pub bits: u32,
    pub nonce: u32,
}

pub struct MempoolEntryDB {
    pub hash: Hash256,
    pub locktime: u32,
    pub fee: i64,
    /// Seconds since the unix epoch when the transaction was first seen.
    pub age: u64,
    /// The chain tip when the transaction was first seen. Eviction is measured
    /// in blocks, not wall-clock: see V13.
    pub seen_height: i32,
    /// Raw transaction bytes. Was hex in a `longtext`, so this is half the size
    /// and TOASTed by PostgreSQL once it passes the page threshold.
    pub tx: Vec<u8>,
}

/// The in-memory sentinel for "this output is not in a block yet".
///
/// Kept so the in-memory UTXO set behaves exactly as before; only the stored
/// representation changes, because the schema says NULL means this and a
/// sentinel in a nullable column would contradict it.
pub const NOT_IN_BLOCK: i32 = -1;

fn height_to_sql(height: i32) -> Option<i32> {
    if height == NOT_IN_BLOCK {
        None
    } else {
        Some(height)
    }
}

/// Reverses [`height_to_sql`] when reading a row back.
pub fn height_from_sql(height: Option<i32>) -> i32 {
    height.unwrap_or(NOT_IN_BLOCK)
}

/// Narrows an unsigned wire value to the signed column that now holds it.
///
/// Returns `None` and logs when it does not fit. Every caller drops the row
/// rather than writing something else: these values are attacker-chosen, and a
/// silent `as` cast would turn an out-of-range height or size into a negative
/// one that reads back as valid.
fn checked<T, U>(value: T, what: &str) -> Option<U>
where
    T: Copy + std::fmt::Display,
    U: TryFrom<T>,
{
    match U::try_from(value) {
        Ok(narrowed) => Some(narrowed),
        Err(_) => {
            log::error!("{what} {value} does not fit its database column; row dropped");
            None
        }
    }
}

// DBOperationType - used to identify the type of operation that the database needs to do

/// One monitor's claim on one outpoint. Many-to-many, hence its own table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorRecord {
    pub txid: Vec<u8>,
    pub vout: u32,
    pub monitor: String,
}

/// One spend, identified by the outpoint it consumes.
///
/// `spending_txid` is deliberately *not* part of the identity. Pre-confirmation
/// a txid is malleable — the same economic spend can be announced under several
/// of them — so every statement that matches an existing row keys on
/// `(txid, vout)` and treats the spending txid as a value to be written, never
/// as a thing to look up by. A settle keyed on the spending txid matches
/// nothing when a malleated sibling is the one that gets mined, and the row
/// stays unsettled permanently.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpendRecord {
    /// The txid of the outpoint being spent.
    pub txid: Vec<u8>,
    pub vout: u32,
    /// The txid of the transaction doing the spending.
    pub spending_txid: Vec<u8>,
}

pub enum DBOperationType {
    UtxoBatchWrite(Vec<UtxoEntryDB>),
    UtxoBatchDelete(Vec<OutPoint>),
    /// A spend seen but not yet mined: move utxo -> utxo_spent, height NULL.
    /// The `i32` is the chain tip when it was seen, recorded in
    /// `utxo_unmined_spend` so eviction can find spends that never confirm.
    UtxoBatchSpend(Vec<SpendRecord>, i32),
    /// Outpoints whose spend never confirmed: move them back to `utxo`.
    UtxoBatchReclaim(Vec<OutPoint>),
    /// A spend seen in a block: settle it at that height.
    UtxoBatchSettle(Vec<SpendRecord>, i32),
    /// Which monitors selected which outpoints.
    UtxoMonitorBatchWrite(Vec<MonitorRecord>),
    TxBatchWrite(Vec<TxEntryWriteDB>),
    MempoolBatchDelete(Vec<Hash256>),
    MempoolBatchWrite(Vec<MempoolEntryDB>),
    BlockHeaderWrite(BlockHeaderWriteDB),
    OrphanBlockHeaderWrite(OrphanBlockHeaderWriteDB),
    BlockHeaderDelete(Hash256),
    TxDelete(u32),
    UtxoDelete(u32),
}

// This will be run in a separate thread that will be responsible for all the database writes
// so as not to delay the main thread of execution during IBD
pub struct Database {
    // Database connection
    conn: PooledConn,
    // Channel on which to receive operations
    rx: mpsc::Receiver<DBOperationType>,

    // Retry database connections
    ms_delay: u64,
    retries: usize,
}

// Upper bound on the number of operations one drain will take off the channel
// before applying them. This bounds the work in a single apply cycle; it does
// not bound memory, because the channel itself is unbounded and anything past
// the cap simply stays queued for the next cycle. Nothing is dropped.
//
// The value is not benchmarked. It is large enough that IBD still coalesces
// into useful batches and small enough that one cycle stays bounded.
const MAX_DRAIN_OPS: usize = 1024;

// Merge adjacent runs of the same batch variant into a single operation.
//
// Order is preserved: only neighbours of the same variant are merged, so a
// delete never moves ahead of a write for the same outpoint. Every operation
// that goes in comes out, in a batch of its own if it cannot be merged.
//
// Coalescing is a throughput optimisation for IBD. Correctness must not depend
// on it, and does not: the result of applying the output is the same as
// applying the input one operation at a time.
fn coalesce_operations(ops: Vec<DBOperationType>) -> Vec<DBOperationType> {
    let mut out: Vec<DBOperationType> = Vec::with_capacity(ops.len());

    for op in ops {
        match (out.last_mut(), op) {
            (Some(DBOperationType::UtxoBatchWrite(acc)), DBOperationType::UtxoBatchWrite(more)) => {
                acc.extend(more)
            }
            (
                Some(DBOperationType::UtxoBatchDelete(acc)),
                DBOperationType::UtxoBatchDelete(more),
            ) => acc.extend(more),
            // Only merged when the seen heights agree, for the same reason
            // settle is: the height is written into utxo_unmined_spend, so two
            // runs at different heights are different statements.
            (
                Some(DBOperationType::UtxoBatchSpend(acc, acc_height)),
                DBOperationType::UtxoBatchSpend(more, more_height),
            ) if *acc_height == more_height => acc.extend(more),
            (
                Some(DBOperationType::UtxoBatchReclaim(acc)),
                DBOperationType::UtxoBatchReclaim(more),
            ) => acc.extend(more),
            (
                Some(DBOperationType::UtxoMonitorBatchWrite(acc)),
                DBOperationType::UtxoMonitorBatchWrite(more),
            ) => acc.extend(more),
            // Only merged when the heights agree. Two settles at different
            // heights are different statements and must stay ordered.
            (
                Some(DBOperationType::UtxoBatchSettle(acc, acc_height)),
                DBOperationType::UtxoBatchSettle(more, more_height),
            ) if *acc_height == more_height => acc.extend(more),
            (Some(DBOperationType::TxBatchWrite(acc)), DBOperationType::TxBatchWrite(more)) => {
                acc.extend(more)
            }
            (
                Some(DBOperationType::MempoolBatchWrite(acc)),
                DBOperationType::MempoolBatchWrite(more),
            ) => acc.extend(more),
            (
                Some(DBOperationType::MempoolBatchDelete(acc)),
                DBOperationType::MempoolBatchDelete(more),
            ) => acc.extend(more),
            (_, op) => out.push(op),
        }
    }

    out
}

/*
Caller should set up channel and pass rx to database
    tx: mpsc::Sender<DBOperationType>,
    let (tx, rx) = mpsc::channel();
    let db = Database::new(conn, rx);
*/

impl Database {
    pub fn new(conn: PooledConn, rx: mpsc::Receiver<DBOperationType>, config: &Config) -> Self {
        // Used to recieve database operations for processing
        Database {
            conn,
            rx,
            ms_delay: config.database.ms_delay,
            retries: config.database.retries,
        }
    }

    fn log_write_error(operation: &str, err: impl std::fmt::Debug) {
        log::error!("Database write failed during {operation}: {err:?}");
    }

    /// Runs `body` inside one transaction, retrying the whole thing.
    ///
    /// The transaction is what makes a batch all-or-nothing. The retry wraps
    /// the transaction rather than sitting inside it, so a retried attempt
    /// starts from a clean slate instead of resuming a transaction the server
    /// has already aborted.
    fn in_transaction<F>(&mut self, operation: &str, body: F)
    where
        F: Fn(&mut postgres::Transaction) -> Result<(), postgres::Error>,
    {
        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || -> Result<(), postgres::Error> {
                let mut tx = self.conn.transaction()?;
                body(&mut tx)?;
                tx.commit()
            },
        );
        if let Err(err) = result {
            Self::log_write_error(operation, err);
        }
    }

    fn utxo_batch_write(&mut self, utxo_entries: Vec<UtxoEntryDB>) {
        if utxo_entries.is_empty() {
            return;
        }
        // Rows are keyed on (txid, vout), which cannot legitimately be written
        // twice with different contents. ON CONFLICT DO UPDATE replaces the
        // REPLACE INTO this used to issue; REPLACE deleted and reinserted,
        // which would have taken the row's identity with it.
        self.in_transaction("utxo batch write", |tx| {
            let stmt = tx.prepare(
                "INSERT INTO utxo (txid, vout, satoshis, locking_script, identifier, created_height) \
                 VALUES ($1, $2, $3, $4, $5, $6) \
                 ON CONFLICT (txid, vout) DO UPDATE \
                 SET satoshis = EXCLUDED.satoshis, \
                     identifier = EXCLUDED.identifier, \
                     created_height = EXCLUDED.created_height",
            )?;
            for entry in &utxo_entries {
                let Some(vout) = checked::<u32, i32>(entry.pos, "output index") else {
                    continue;
                };
                tx.execute(
                    &stmt,
                    &[
                        &entry.hash,
                        &vout,
                        &entry.satoshis,
                        &entry.locking_script,
                        &entry.identifier,
                        &height_to_sql(entry.height),
                    ],
                )?;
            }
            Ok(())
        });
    }

    fn utxo_batch_delete(&mut self, utxo_deletes: Vec<OutPoint>) {
        if utxo_deletes.is_empty() {
            return;
        }
        self.in_transaction("utxo batch delete", |tx| {
            let stmt = tx.prepare("DELETE FROM utxo WHERE txid = $1 AND vout = $2")?;
            for outpoint in &utxo_deletes {
                let Some(vout) = checked::<u32, i32>(outpoint.index, "output index") else {
                    continue;
                };
                tx.execute(&stmt, &[&&outpoint.hash.0[..], &vout])?;
            }
            Ok(())
        });
    }

    /// Splits spends into the three parallel arrays the set-based statements
    /// take, dropping any whose vout will not fit the signed column.
    fn spend_arrays(spends: &[SpendRecord]) -> (Vec<&[u8]>, Vec<i32>, Vec<&[u8]>) {
        let mut txids = Vec::with_capacity(spends.len());
        let mut vouts = Vec::with_capacity(spends.len());
        let mut spending = Vec::with_capacity(spends.len());
        for spend in spends {
            let Some(vout) = checked::<u32, i32>(spend.vout, "output index") else {
                continue;
            };
            txids.push(spend.txid.as_slice());
            vouts.push(vout);
            spending.push(spend.spending_txid.as_slice());
        }
        (txids, vouts, spending)
    }

    /// A spend seen but not yet mined: the row moves out of the spendable set
    /// and into `utxo_spent` with a NULL `spent_height`.
    ///
    /// `ON CONFLICT DO NOTHING` rather than an update: a second, conflicting
    /// spend of the same outpoint must not overwrite the first sighting. First
    /// seen wins, and distinguishing a malleated sibling from a genuine
    /// double-spend is CS-428, not this.
    fn utxo_batch_spend(&mut self, spends: Vec<SpendRecord>, seen_height: i32) {
        if spends.is_empty() {
            return;
        }
        self.in_transaction("utxo batch spend", |tx| {
            let (txids, vouts, spending) = Self::spend_arrays(&spends);
            if txids.is_empty() {
                return Ok(());
            }
            // One statement, three stages, because the last must record only
            // what the first two actually did.
            //
            // Written as two statements first, and that was wrong: the second
            // inserted a row for every outpoint in the batch, including ones
            // the DELETE never found and ones the ON CONFLICT declined. Those
            // rows name an outpoint with no `utxo_spent` row, so the eviction
            // join never matches them and nothing ever deletes them — a
            // permanent leak of exactly the kind this ticket exists to remove,
            // in a brand new table. Chaining off `spent`'s RETURNING makes the
            // record of a sighting conditional on the sighting being stored.
            //
            // Keyed on the outpoint, so a malleated re-announcement of the same
            // spend updates nothing and the original `seen_height` stands: the
            // age of a spend is the age of the *attempt*, and letting a
            // rebroadcast reset it would make an outpoint unreclaimable.
            tx.execute(
                "WITH moved AS ( \
                     DELETE FROM utxo u \
                     USING unnest($1::bytea[], $2::integer[], $3::bytea[]) \
                          AS i(txid, vout, spending_txid) \
                     WHERE u.txid = i.txid AND u.vout = i.vout \
                     RETURNING u.txid, u.vout, u.satoshis, u.locking_script, \
                               u.identifier, u.created_height, i.spending_txid \
                 ), spent AS ( \
                     INSERT INTO utxo_spent (txid, vout, satoshis, locking_script, \
                                             identifier, created_height, spent_txid, \
                                             spent_height) \
                     SELECT txid, vout, satoshis, locking_script, identifier, \
                            created_height, spending_txid, NULL \
                     FROM moved \
                     ON CONFLICT (txid, vout) DO NOTHING \
                     RETURNING txid, vout \
                 ) \
                 INSERT INTO utxo_unmined_spend (txid, vout, seen_height) \
                 SELECT txid, vout, $4 FROM spent \
                 ON CONFLICT (txid, vout) DO NOTHING",
                &[&txids, &vouts, &spending, &seen_height],
            )?;
            Ok(())
        });
    }

    /// Outpoints whose spend was seen but never mined, returned to the
    /// spendable set.
    ///
    /// Keyed on the outpoint, never the spending txid, for the same reason the
    /// settle is.
    ///
    /// `spent_height IS NULL` is re-asserted here rather than trusted from the
    /// caller's earlier SELECT. Operations reach this thread in order, so a
    /// settle queued before the reclaim has already run by the time this
    /// executes; without the guard, a spend that confirmed in the interval
    /// would be un-settled and handed back as spendable.
    fn utxo_batch_reclaim(&mut self, outpoints: Vec<OutPoint>) {
        if outpoints.is_empty() {
            return;
        }
        self.in_transaction("utxo batch reclaim", |tx| {
            let mut txids: Vec<&[u8]> = Vec::with_capacity(outpoints.len());
            let mut vouts: Vec<i32> = Vec::with_capacity(outpoints.len());
            for outpoint in &outpoints {
                let Some(vout) = checked::<u32, i32>(outpoint.index, "output index") else {
                    continue;
                };
                txids.push(&outpoint.hash.0[..]);
                vouts.push(vout);
            }
            if txids.is_empty() {
                return Ok(());
            }

            tx.execute(
                "WITH moved AS ( \
                     DELETE FROM utxo_spent s \
                     USING unnest($1::bytea[], $2::integer[]) AS i(txid, vout) \
                     WHERE s.txid = i.txid AND s.vout = i.vout \
                       AND s.spent_height IS NULL \
                     RETURNING s.txid, s.vout, s.satoshis, s.locking_script, \
                               s.identifier, s.created_height \
                 ) \
                 INSERT INTO utxo (txid, vout, satoshis, locking_script, \
                                   identifier, created_height) \
                 SELECT txid, vout, satoshis, locking_script, identifier, \
                        created_height \
                 FROM moved \
                 ON CONFLICT (txid, vout) DO NOTHING",
                &[&txids, &vouts],
            )?;

            // Unconditional: whether or not the row above moved, this outpoint
            // is no longer an unmined spend awaiting a decision. If the settle
            // won the race the row is settled and must not be revisited.
            tx.execute(
                "DELETE FROM utxo_unmined_spend u \
                 USING unnest($1::bytea[], $2::integer[]) AS i(txid, vout) \
                 WHERE u.txid = i.txid AND u.vout = i.vout",
                &[&txids, &vouts],
            )?;
            Ok(())
        });
    }

    /// A spend seen in a block, at `height`.
    ///
    /// Two statements, in this order, because a spend reaches a block by one of
    /// two routes and only one of them has a row already:
    ///
    /// 1. it was seen in the mempool first, so `utxo_spent` holds it with a
    ///    NULL height — settle that row in place;
    /// 2. it was never seen, so the outpoint is still in `utxo` — move it,
    ///    already settled.
    ///
    /// **Both key on the outpoint, never on the spending txid.** A spend
    /// announced as one txid and mined as a malleated sibling under another is
    /// the same spend; keyed on the txid, statement 1 matches nothing and the
    /// row keeps its NULL height for ever. Keyed on the outpoint it settles,
    /// and `spent_txid` is corrected to the txid the block actually carried.
    fn utxo_batch_settle(&mut self, spends: Vec<SpendRecord>, height: i32) {
        if spends.is_empty() {
            return;
        }
        self.in_transaction("utxo batch settle", |tx| {
            let (txids, vouts, spending) = Self::spend_arrays(&spends);
            if txids.is_empty() {
                return Ok(());
            }

            // 1. Settle spends already recorded from the mempool.
            tx.execute(
                "UPDATE utxo_spent s \
                 SET spent_height = $4, spent_txid = i.spending_txid \
                 FROM unnest($1::bytea[], $2::integer[], $3::bytea[]) \
                      AS i(txid, vout, spending_txid) \
                 WHERE s.txid = i.txid AND s.vout = i.vout AND s.spent_height IS NULL",
                &[&txids, &vouts, &spending, &height],
            )?;

            // 2. Move anything still live — the spend was never seen unmined.
            tx.execute(
                "WITH moved AS ( \
                     DELETE FROM utxo u \
                     USING unnest($1::bytea[], $2::integer[], $3::bytea[]) \
                          AS i(txid, vout, spending_txid) \
                     WHERE u.txid = i.txid AND u.vout = i.vout \
                     RETURNING u.txid, u.vout, u.satoshis, u.locking_script, \
                               u.identifier, u.created_height, i.spending_txid \
                 ) \
                 INSERT INTO utxo_spent (txid, vout, satoshis, locking_script, \
                                         identifier, created_height, spent_txid, spent_height) \
                 SELECT txid, vout, satoshis, locking_script, identifier, \
                        created_height, spending_txid, $4 \
                 FROM moved \
                 ON CONFLICT (txid, vout) DO NOTHING",
                &[&txids, &vouts, &spending, &height],
            )?;

            // 3. Whichever route it took, the spend is mined, so it is no
            //    longer a candidate for eviction. Keyed on the outpoint, so a
            //    spend mined under a malleated txid still clears the record
            //    the original announcement left.
            tx.execute(
                "DELETE FROM utxo_unmined_spend u \
                 USING unnest($1::bytea[], $2::integer[]) AS i(txid, vout) \
                 WHERE u.txid = i.txid AND u.vout = i.vout",
                &[&txids, &vouts],
            )?;
            Ok(())
        });
    }

    /// Which monitors selected which outpoints.
    ///
    /// `ON CONFLICT DO NOTHING` because seeing the same output selected by the
    /// same monitor twice is ordinary — a reorg replays it — rather than an
    /// error.
    fn utxo_monitor_batch_write(&mut self, records: Vec<MonitorRecord>) {
        if records.is_empty() {
            return;
        }
        self.in_transaction("utxo monitor batch write", |tx| {
            let stmt = tx.prepare(
                "INSERT INTO utxo_monitor (txid, vout, monitor) VALUES ($1, $2, $3) \
                 ON CONFLICT (txid, vout, monitor) DO NOTHING",
            )?;
            for record in &records {
                let Some(vout) = checked::<u32, i32>(record.vout, "output index") else {
                    continue;
                };
                tx.execute(&stmt, &[&record.txid, &vout, &record.monitor])?;
            }
            Ok(())
        });
    }

    fn tx_batch_write(&mut self, tx_entries: Vec<TxEntryWriteDB>) {
        if tx_entries.is_empty() {
            return;
        }
        self.in_transaction("tx batch write", |db| {
            let stmt = db.prepare(
                "INSERT INTO tx (hash, height, blockindex, txsize, satoshis) \
                 VALUES ($1, $2, $3, $4, $5) \
                 ON CONFLICT (hash) DO NOTHING",
            )?;
            for entry in &tx_entries {
                let (Some(height), Some(blockindex), Some(txsize), Some(satoshis)) = (
                    checked::<usize, i32>(entry.height, "block height"),
                    checked::<u32, i32>(entry.blockindex, "block index"),
                    checked::<u32, i32>(entry.size, "transaction size"),
                    checked::<u64, i64>(entry.satoshis, "transaction satoshis"),
                ) else {
                    continue;
                };
                db.execute(
                    &stmt,
                    &[&&entry.hash.0[..], &height, &blockindex, &txsize, &satoshis],
                )?;
            }
            Ok(())
        });
    }

    fn mempool_batch_write(&mut self, mempool_entries: Vec<MempoolEntryDB>) {
        if mempool_entries.is_empty() {
            return;
        }
        self.in_transaction("mempool batch write", |db| {
            let stmt = db.prepare(
                "INSERT INTO mempool (hash, locktime, fee, seen_at, tx, seen_height) \
                 VALUES ($1, $2, $3, $4, $5, $6) \
                 ON CONFLICT (hash) DO NOTHING",
            )?;
            for entry in &mempool_entries {
                // `seen_at` is a timestamptz; the old column was an `int
                // unsigned` unix timestamp that overflows in 2106.
                let seen_at = UNIX_EPOCH + Duration::from_secs(entry.age);
                let locktime = i64::from(entry.locktime);
                db.execute(
                    &stmt,
                    &[
                        &&entry.hash.0[..],
                        &locktime,
                        &entry.fee,
                        &seen_at,
                        &entry.tx,
                        &entry.seen_height,
                    ],
                )?;
            }
            Ok(())
        });
    }

    fn mempool_batch_delete(&mut self, mempool_hashes: Vec<Hash256>) {
        if mempool_hashes.is_empty() {
            return;
        }
        self.in_transaction("mempool batch delete", |db| {
            let stmt = db.prepare("DELETE FROM mempool WHERE hash = $1")?;
            for hash in &mempool_hashes {
                db.execute(&stmt, &[&&hash.0[..]])?;
            }
            Ok(())
        });
    }

    fn block_header_write(&mut self, block_header: BlockHeaderWriteDB) {
        // Every one of these is an unsigned wire value going into a signed
        // column, so they are narrowed together and the row is dropped whole if
        // any of them does not fit.
        let (
            Some(height),
            Some(version),
            Some(block_time),
            Some(bits),
            Some(nonce),
            Some(file_offset),
            Some(blocksize),
            Some(numtxs),
        ) = (
            checked::<u32, i32>(block_header.height, "block height"),
            checked::<u32, i32>(block_header.version, "block version"),
            checked::<u32, i32>(block_header.timestamp, "block timestamp"),
            checked::<u32, i32>(block_header.bits, "block bits"),
            checked::<u32, i32>(block_header.nonce, "block nonce"),
            checked::<u64, i64>(block_header.position, "block file offset"),
            checked::<u32, i32>(block_header.blocksize, "block size"),
            checked::<u32, i32>(block_header.numtxs, "block transaction count"),
        )
        else {
            return;
        };

        self.in_transaction("block header write", |db| {
            let params: [&(dyn ToSql + Sync); 11] = [
                &height,
                &&block_header.hash.0[..],
                &version,
                &&block_header.prev_hash.0[..],
                &&block_header.merkle_root.0[..],
                &block_time,
                &bits,
                &nonce,
                &file_offset,
                &blocksize,
                &numtxs,
            ];
            db.execute(
                "INSERT INTO blocks \
                 (height, hash, version, prev_hash, merkle_root, block_time, bits, nonce, \
                  file_offset, blocksize, numtxs) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
                 ON CONFLICT (hash) DO NOTHING",
                &params,
            )?;
            Ok(())
        });
    }

    fn block_header_delete(&mut self, hash: &Hash256) {
        let hash = hash.0;
        self.in_transaction("block header delete", move |db| {
            db.execute("DELETE FROM blocks WHERE hash = $1", &[&&hash[..]])?;
            Ok(())
        });
    }

    fn tx_delete_at_height(&mut self, height: u32) {
        let Some(height) = checked::<u32, i32>(height, "block height") else {
            return;
        };
        self.in_transaction("tx delete at height", move |db| {
            db.execute("DELETE FROM tx WHERE height = $1", &[&height])?;
            Ok(())
        });
    }

    fn utxo_delete_at_height(&mut self, height: u32) {
        let Some(height) = checked::<u32, i32>(height, "block height") else {
            return;
        };
        self.in_transaction("utxo delete at height", move |db| {
            db.execute("DELETE FROM utxo WHERE created_height = $1", &[&height])?;
            Ok(())
        });
    }

    fn orphan_block_header_write(&mut self, block_header: OrphanBlockHeaderWriteDB) {
        let (Some(height), Some(version), Some(block_time), Some(bits), Some(nonce)) = (
            checked::<u32, i32>(block_header.height, "orphan height"),
            checked::<u32, i32>(block_header.version, "orphan version"),
            checked::<u32, i32>(block_header.timestamp, "orphan timestamp"),
            checked::<u32, i32>(block_header.bits, "orphan bits"),
            checked::<u32, i32>(block_header.nonce, "orphan nonce"),
        ) else {
            return;
        };

        self.in_transaction("orphan block header write", |db| {
            let params: [&(dyn ToSql + Sync); 8] = [
                &height,
                &&block_header.hash.0[..],
                &version,
                &&block_header.prev_hash.0[..],
                &&block_header.merkle_root.0[..],
                &block_time,
                &bits,
                &nonce,
            ];
            db.execute(
                "INSERT INTO orphans \
                 (height, hash, version, prev_hash, merkle_root, block_time, bits, nonce) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
                 ON CONFLICT (hash) DO NOTHING",
                &params,
            )?;
            Ok(())
        });
    }

    fn apply(&mut self, op: DBOperationType) {
        match op {
            DBOperationType::UtxoBatchWrite(entries) => self.utxo_batch_write(entries),
            DBOperationType::UtxoBatchDelete(deletes) => self.utxo_batch_delete(deletes),
            DBOperationType::UtxoBatchSpend(spends, seen_height) => {
                self.utxo_batch_spend(spends, seen_height)
            }
            DBOperationType::UtxoBatchReclaim(outpoints) => self.utxo_batch_reclaim(outpoints),
            DBOperationType::UtxoMonitorBatchWrite(records) => {
                self.utxo_monitor_batch_write(records)
            }
            DBOperationType::UtxoBatchSettle(spends, height) => {
                self.utxo_batch_settle(spends, height)
            }
            DBOperationType::TxBatchWrite(entries) => self.tx_batch_write(entries),
            DBOperationType::MempoolBatchWrite(entries) => self.mempool_batch_write(entries),
            DBOperationType::MempoolBatchDelete(hashes) => self.mempool_batch_delete(hashes),
            DBOperationType::BlockHeaderWrite(block_header) => {
                self.block_header_write(block_header)
            }
            DBOperationType::OrphanBlockHeaderWrite(block_header) => {
                self.orphan_block_header_write(block_header)
            }
            DBOperationType::BlockHeaderDelete(hash) => self.block_header_delete(&hash),
            DBOperationType::TxDelete(height) => self.tx_delete_at_height(height),
            DBOperationType::UtxoDelete(height) => self.utxo_delete_at_height(height),
        }
    }

    // Take everything the channel has queued right now, blocking for the first
    // operation. Returns an empty Vec only when the channel has closed, which
    // ends perform_db_operations.
    fn drain_pending(&mut self) -> Vec<DBOperationType> {
        let Ok(first) = self.rx.recv() else {
            return Vec::new();
        };
        let mut ops = vec![first];
        while ops.len() < MAX_DRAIN_OPS {
            match self.rx.try_recv() {
                Ok(op) => ops.push(op),
                Err(_) => break,
            }
        }
        ops
    }

    pub fn perform_db_operations(&mut self) {
        loop {
            let ops = self.drain_pending();
            if ops.is_empty() {
                return;
            }
            for op in coalesce_operations(ops) {
                self.apply(op);
            }
        }
    }
}
#[cfg(test)]
mod test {
    use super::*;
    use std::sync::mpsc;

    // Every fixture operation carries a u32 id in whichever field is free.
    // ids() reads them back, so a single assertion covers both "nothing was
    // dropped" and "nothing was reordered".
    fn hash_of(id: u32) -> Hash256 {
        let mut bytes = [0u8; 32];
        bytes[..4].copy_from_slice(&id.to_le_bytes());
        Hash256(bytes)
    }

    fn id_of_hash(hash: &Hash256) -> u32 {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&hash.0[..4]);
        u32::from_le_bytes(bytes)
    }

    fn utxo_write(ids: &[u32]) -> DBOperationType {
        DBOperationType::UtxoBatchWrite(
            ids.iter()
                .map(|id| UtxoEntryDB {
                    hash: hash_of(*id).0.to_vec(),
                    pos: *id,
                    satoshis: 1_000,
                    height: 1,
                    locking_script: vec![0x76, 0xa9, 0x14],
                    identifier: None,
                })
                .collect(),
        )
    }

    fn utxo_delete(ids: &[u32]) -> DBOperationType {
        DBOperationType::UtxoBatchDelete(
            ids.iter()
                .map(|id| OutPoint {
                    hash: hash_of(*id),
                    index: *id,
                })
                .collect(),
        )
    }

    fn tx_write(ids: &[u32]) -> DBOperationType {
        DBOperationType::TxBatchWrite(
            ids.iter()
                .map(|id| TxEntryWriteDB {
                    hash: hash_of(*id),
                    height: 1,
                    blockindex: *id,
                    size: 100,
                    satoshis: 1_000,
                })
                .collect(),
        )
    }

    fn mempool_write(ids: &[u32]) -> DBOperationType {
        DBOperationType::MempoolBatchWrite(
            ids.iter()
                .map(|id| MempoolEntryDB {
                    hash: hash_of(*id),
                    locktime: *id,
                    fee: 0,
                    age: 0,
                    seen_height: 0,
                    tx: Vec::new(),
                })
                .collect(),
        )
    }

    fn mempool_delete(ids: &[u32]) -> DBOperationType {
        DBOperationType::MempoolBatchDelete(ids.iter().map(|id| hash_of(*id)).collect())
    }

    // Flatten a run of operations to the id sequence they would apply.
    fn ids(ops: &[DBOperationType]) -> Vec<u32> {
        let mut out = Vec::new();
        for op in ops {
            match op {
                DBOperationType::UtxoBatchWrite(entries) => {
                    out.extend(entries.iter().map(|e| e.pos))
                }
                DBOperationType::UtxoBatchDelete(deletes) => {
                    out.extend(deletes.iter().map(|d| d.index))
                }
                DBOperationType::UtxoBatchSpend(spends, _) => {
                    out.extend(spends.iter().map(|s| s.vout))
                }
                DBOperationType::UtxoBatchReclaim(outpoints) => {
                    out.extend(outpoints.iter().map(|o| o.index))
                }
                DBOperationType::UtxoMonitorBatchWrite(records) => {
                    out.extend(records.iter().map(|r| r.vout))
                }
                DBOperationType::UtxoBatchSettle(spends, _) => {
                    out.extend(spends.iter().map(|s| s.vout))
                }
                DBOperationType::TxBatchWrite(entries) => {
                    out.extend(entries.iter().map(|e| e.blockindex))
                }
                DBOperationType::MempoolBatchWrite(entries) => {
                    out.extend(entries.iter().map(|e| e.locktime))
                }
                DBOperationType::MempoolBatchDelete(hashes) => {
                    out.extend(hashes.iter().map(id_of_hash))
                }
                DBOperationType::BlockHeaderWrite(header) => out.push(header.height),
                DBOperationType::OrphanBlockHeaderWrite(header) => out.push(header.height),
                DBOperationType::BlockHeaderDelete(hash) => out.push(id_of_hash(hash)),
                DBOperationType::TxDelete(height) => out.push(*height),
                DBOperationType::UtxoDelete(height) => out.push(*height),
            }
        }
        out
    }

    // Variant labels, so a test can assert how the run was grouped as well as
    // what it contains.
    fn shape(ops: &[DBOperationType]) -> Vec<&'static str> {
        ops.iter()
            .map(|op| match op {
                DBOperationType::UtxoBatchWrite(_) => "utxo_write",
                DBOperationType::UtxoBatchDelete(_) => "utxo_delete",
                DBOperationType::UtxoBatchSpend(_, _) => "utxo_spend",
                DBOperationType::UtxoBatchReclaim(_) => "utxo_reclaim",
                DBOperationType::UtxoMonitorBatchWrite(_) => "utxo_monitor_write",
                DBOperationType::UtxoBatchSettle(_, _) => "utxo_settle",
                DBOperationType::TxBatchWrite(_) => "tx_write",
                DBOperationType::MempoolBatchWrite(_) => "mempool_write",
                DBOperationType::MempoolBatchDelete(_) => "mempool_delete",
                DBOperationType::BlockHeaderWrite(_) => "block_header_write",
                DBOperationType::OrphanBlockHeaderWrite(_) => "orphan_write",
                DBOperationType::BlockHeaderDelete(_) => "block_header_delete",
                DBOperationType::TxDelete(_) => "tx_delete_height",
                DBOperationType::UtxoDelete(_) => "utxo_delete_height",
            })
            .collect()
    }

    // The exact sequence Utxo::update_db sends, plus a trailing TxDelete. Under
    // the old coalescers try_recv() popped the UtxoBatchDelete, failed the
    // UtxoBatchWrite pattern, and dropped it on the floor.
    #[test]
    fn db01_write_delete_and_txdelete_all_survive() {
        let ops = vec![
            utxo_write(&[1, 2]),
            utxo_delete(&[3]),
            DBOperationType::TxDelete(4),
        ];

        let result = coalesce_operations(ops);

        assert_eq!(
            shape(&result),
            vec!["utxo_write", "utxo_delete", "tx_delete_height"],
            "differing variants must stay separate operations"
        );
        assert_eq!(
            ids(&result),
            vec![1, 2, 3, 4],
            "no operation may be dropped"
        );
    }

    #[test]
    fn db02_alternating_write_and_delete_batches_all_survive() {
        let mut ops = Vec::new();
        let mut expected = Vec::new();
        for i in 0..100u32 {
            ops.push(utxo_write(&[i * 2]));
            ops.push(utxo_delete(&[i * 2 + 1]));
            expected.push(i * 2);
            expected.push(i * 2 + 1);
        }

        let result = coalesce_operations(ops);

        assert_eq!(ids(&result), expected, "all 200 entries must be applied");
        assert_eq!(
            result.len(),
            200,
            "alternating variants cannot be merged, so nothing should coalesce"
        );
    }

    // A delete overtaking a write for the same outpoint would resurrect a spent
    // output, so relative order between differing variants is load-bearing.
    #[test]
    fn db03_adjacent_same_variant_runs_merge_and_order_is_preserved() {
        let ops = vec![
            utxo_write(&[1]),
            utxo_write(&[2]),
            utxo_write(&[3]),
            utxo_delete(&[4]),
            utxo_delete(&[5]),
            utxo_write(&[6]),
        ];

        let result = coalesce_operations(ops);

        assert_eq!(
            shape(&result),
            vec!["utxo_write", "utxo_delete", "utxo_write"],
            "three runs in, three operations out"
        );
        assert_eq!(ids(&result), vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn db04_every_variant_round_trips_in_order() {
        let ops = vec![
            utxo_write(&[1]),
            utxo_delete(&[2]),
            tx_write(&[3]),
            mempool_write(&[4]),
            mempool_delete(&[5]),
            DBOperationType::BlockHeaderWrite(BlockHeaderWriteDB {
                height: 6,
                hash: hash_of(6),
                version: 1,
                prev_hash: hash_of(0),
                merkle_root: hash_of(0),
                timestamp: 0,
                bits: 0,
                nonce: 0,
                position: 0,
                blocksize: 0,
                numtxs: 0,
            }),
            DBOperationType::OrphanBlockHeaderWrite(OrphanBlockHeaderWriteDB {
                height: 7,
                ..OrphanBlockHeaderWriteDB::default()
            }),
            DBOperationType::BlockHeaderDelete(hash_of(8)),
            DBOperationType::TxDelete(9),
            DBOperationType::UtxoDelete(10),
        ];
        let expected_shape = shape(&ops);
        let expected_ids = ids(&ops);

        let result = coalesce_operations(ops);

        assert_eq!(
            shape(&result),
            expected_shape,
            "no two differing variants may be merged"
        );
        assert_eq!(expected_ids, ids(&result));
    }

    // Coalescing is a throughput optimisation. The applied result must be the
    // same when it never fires.
    #[test]
    fn db05_batches_of_one_are_unchanged() {
        let ops = vec![
            utxo_write(&[1]),
            mempool_delete(&[2]),
            mempool_write(&[3]),
            tx_write(&[4]),
        ];
        let expected_shape = shape(&ops);
        let expected_ids = ids(&ops);

        let result = coalesce_operations(ops);

        assert_eq!(shape(&result), expected_shape);
        assert_eq!(ids(&result), expected_ids);
    }

    #[test]
    fn db06_empty_input_produces_no_operations() {
        assert!(coalesce_operations(Vec::new()).is_empty());
    }

    // End to end over the real writer loop: the coalescer, the dispatch and the
    // SQL. perform_db_operations returns once the sender is dropped and the
    // channel drains, so the assertions run after every operation is applied.
    //
    // Needs a PostgreSQL server with the schema applied, and is skipped rather
    // than failed without one.
    #[test]
    fn db07_delete_between_writes_reaches_the_database() {
        let Some(pool) = crate::db::shared_test_pool() else {
            eprintln!(
                "skipping db07_delete_between_writes_reaches_the_database: \
                 UAAS_TEST_POSTGRES_URL not set"
            );
            return;
        };
        let mut setup = pool.get().expect("get connection for fixture setup");

        // The table is the migrations' now; this test no longer creates it. If
        // it is absent the migrations have not been applied, and failing here
        // says so more usefully than a CREATE TABLE that papers over it.
        setup
            .execute("SELECT 1 FROM utxo WHERE false", &[])
            .expect("utxo table must exist -- run `uaas migrate` against the test database");

        // Ids well clear of the other tests' rows, and inside i32 — vout is a
        // signed `integer` column now, so the 0xdb00_0001 these used to use is
        // out of range and utxo_batch_write correctly refuses it.
        let spent = 0x0db0_0001u32;
        let kept = 0x0db0_0002u32;
        for id in [spent, kept] {
            setup
                .execute("DELETE FROM utxo WHERE txid = $1", &[&&hash_of(id).0[..]])
                .expect("clear fixture rows");
        }

        let (tx, rx) = mpsc::channel();
        // The losing sequence: a delete sandwiched between two writes. The old
        // coalescers consumed the delete while accumulating the first write and
        // never applied it.
        tx.send(utxo_write(&[spent])).expect("send write");
        tx.send(utxo_delete(&[spent])).expect("send delete");
        tx.send(utxo_write(&[kept])).expect("send second write");
        drop(tx);

        let mut database = Database {
            conn: pool.get().expect("get connection for writer"),
            rx,
            ms_delay: 300,
            retries: 3,
        };
        database.perform_db_operations();

        let present = |conn: &mut crate::db::PooledConn, id: u32| -> bool {
            let vout = i32::try_from(id).expect("fixture id fits an i32");
            conn.query_opt(
                "SELECT vout FROM utxo WHERE txid = $1 AND vout = $2",
                &[&&hash_of(id).0[..], &vout],
            )
            .expect("query fixture row")
            .is_some()
        };

        let mut check = pool.get().expect("get connection for assertions");
        assert!(
            !present(&mut check, spent),
            "the delete between two writes must have reached the database"
        );
        assert!(
            present(&mut check, kept),
            "the write after the delete must have reached the database"
        );

        for id in [spent, kept] {
            check
                .execute("DELETE FROM utxo WHERE txid = $1", &[&&hash_of(id).0[..]])
                .expect("clean up fixture rows");
        }
    }

    // --- CS-421: the move-and-settle -------------------------------------
    //
    // These need a PostgreSQL server with the schema applied, and are skipped
    // rather than failed without one.

    /// A live utxo row, and the spend of it, over ids well clear of the other
    /// tests'. Returns the outpoint's parts so assertions can name them.
    fn spend_fixture(id: u32, spender: u32) -> (Vec<u8>, i32, Vec<u8>) {
        (
            hash_of(id).0.to_vec(),
            i32::try_from(id).expect("fixture id fits an i32"),
            hash_of(spender).0.to_vec(),
        )
    }

    fn seed_live_utxo(conn: &mut crate::db::PooledConn, txid: &[u8], vout: i32) {
        conn.execute("DELETE FROM utxo_spent WHERE txid = $1", &[&txid])
            .expect("clear utxo_spent fixture");
        conn.execute("DELETE FROM utxo WHERE txid = $1", &[&txid])
            .expect("clear utxo fixture");
        conn.execute(
            "INSERT INTO utxo (txid, vout, satoshis, locking_script, identifier, created_height) \
             VALUES ($1, $2, 1000, '\\x76a914', NULL, 300)",
            &[&txid, &vout],
        )
        .expect("seed a live utxo row");
    }

    fn settled_state(
        conn: &mut crate::db::PooledConn,
        txid: &[u8],
    ) -> Option<(Vec<u8>, Option<i32>)> {
        conn.query_opt(
            "SELECT spent_txid, spent_height FROM utxo_spent WHERE txid = $1",
            &[&txid],
        )
        .expect("read utxo_spent")
        .map(|row| (row.get(0), row.get(1)))
    }

    fn live_count(conn: &mut crate::db::PooledConn, txid: &[u8]) -> i64 {
        conn.query_one("SELECT count(*) FROM utxo WHERE txid = $1", &[&txid])
            .expect("count live rows")
            .get(0)
    }

    fn database_for(pool: &crate::db::Pool) -> Database {
        let (_tx, rx) = mpsc::channel();
        Database {
            conn: pool.get().expect("connection for the writer"),
            rx,
            ms_delay: 300,
            retries: 3,
        }
    }

    #[test]
    fn db08_a_spend_seen_unmined_moves_the_row_and_leaves_the_height_null() {
        let Some(pool) = crate::db::shared_test_pool() else {
            eprintln!("skipping db08: UAAS_TEST_POSTGRES_URL not set");
            return;
        };
        let mut conn = pool.get().expect("fixture connection");

        let (txid, vout, spender) = spend_fixture(0x0db1_0001, 0x0db1_00a1);
        seed_live_utxo(&mut conn, &txid, vout);

        let mut database = database_for(&pool);
        database.utxo_batch_spend(
            vec![SpendRecord {
                txid: txid.clone(),
                vout: u32::try_from(vout).expect("vout fits"),
                spending_txid: spender.clone(),
            }],
            0,
        );

        assert_eq!(
            live_count(&mut conn, &txid),
            0,
            "the outpoint must leave the spendable set"
        );
        assert_eq!(
            settled_state(&mut conn, &txid),
            Some((spender, None)),
            "it must be recorded as spent but unmined"
        );
    }

    // The reason every statement keys on the outpoint.
    //
    // A spend is announced as txid A and reaches the mempool. The block then
    // carries its malleated sibling B — same outpoint, same outputs, different
    // unlocking script, different txid. Keyed on the outpoint the row settles
    // and spent_txid is corrected to B. Keyed on the spending txid it would
    // match nothing and keep its NULL height for ever, which is the failure
    // this test exists to make impossible to reintroduce.
    #[test]
    fn db09_the_settle_is_keyed_on_the_outpoint_not_the_spending_txid() {
        let Some(pool) = crate::db::shared_test_pool() else {
            eprintln!("skipping db09: UAAS_TEST_POSTGRES_URL not set");
            return;
        };
        let mut conn = pool.get().expect("fixture connection");

        let (txid, vout, announced) = spend_fixture(0x0db1_0002, 0x0db1_00a2);
        // The sibling that actually gets mined, under a different txid.
        let mined = hash_of(0x0db1_00b2).0.to_vec();
        assert_ne!(announced, mined, "the siblings must have different txids");

        seed_live_utxo(&mut conn, &txid, vout);

        let mut database = database_for(&pool);
        let vout_u32 = u32::try_from(vout).expect("vout fits");

        // Seen in the mempool as A.
        database.utxo_batch_spend(
            vec![SpendRecord {
                txid: txid.clone(),
                vout: vout_u32,
                spending_txid: announced.clone(),
            }],
            0,
        );
        assert_eq!(
            settled_state(&mut conn, &txid),
            Some((announced, None)),
            "the mempool sighting is recorded unmined"
        );

        // Mined as B.
        database.utxo_batch_settle(
            vec![SpendRecord {
                txid: txid.clone(),
                vout: vout_u32,
                spending_txid: mined.clone(),
            }],
            301,
        );

        assert_eq!(
            settled_state(&mut conn, &txid),
            Some((mined, Some(301))),
            "the settle must match on the outpoint and correct the spending txid"
        );
    }

    // The other route into a block: the spend was never seen unmined, so the
    // outpoint is still live and statement 2 moves it already settled.
    #[test]
    fn db10_a_spend_never_seen_unmined_is_moved_and_settled_in_one_step() {
        let Some(pool) = crate::db::shared_test_pool() else {
            eprintln!("skipping db10: UAAS_TEST_POSTGRES_URL not set");
            return;
        };
        let mut conn = pool.get().expect("fixture connection");

        let (txid, vout, spender) = spend_fixture(0x0db1_0003, 0x0db1_00a3);
        seed_live_utxo(&mut conn, &txid, vout);

        let mut database = database_for(&pool);
        database.utxo_batch_settle(
            vec![SpendRecord {
                txid: txid.clone(),
                vout: u32::try_from(vout).expect("vout fits"),
                spending_txid: spender.clone(),
            }],
            302,
        );

        assert_eq!(live_count(&mut conn, &txid), 0, "the row must move");
        assert_eq!(
            settled_state(&mut conn, &txid),
            Some((spender, Some(302))),
            "and arrive already settled at the block height"
        );
    }

    #[test]
    fn test_operation() {
        let Some(pool) = crate::db::shared_test_pool() else {
            eprintln!("skipping database integration test: UAAS_TEST_POSTGRES_URL not set");
            return;
        };
        let conn = pool
            .get()
            .expect("get connection for database integration test");
        let (_tx, rx) = mpsc::channel();
        let mut database = Database {
            conn,
            rx,
            ms_delay: 300,
            retries: 3,
        };

        let block_header: OrphanBlockHeaderWriteDB = OrphanBlockHeaderWriteDB::default();

        database.orphan_block_header_write(block_header);

        //assert_eq!(datetime.timestamp(), 1684477516);
    }

    // --- CS-423: mempool eviction ----------------------------------------

    fn unmined_seen_height(conn: &mut crate::db::PooledConn, txid: &[u8]) -> Option<i32> {
        conn.query_opt(
            "SELECT seen_height FROM utxo_unmined_spend WHERE txid = $1",
            &[&txid],
        )
        .expect("read utxo_unmined_spend")
        .map(|row| row.get(0))
    }

    fn outpoint_of(txid: &[u8], vout: i32) -> OutPoint {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(txid);
        OutPoint {
            hash: Hash256(bytes),
            index: u32::try_from(vout).expect("vout fits"),
        }
    }

    fn spend_record(txid: &[u8], vout: i32, spender: &[u8]) -> SpendRecord {
        SpendRecord {
            txid: txid.to_vec(),
            vout: u32::try_from(vout).expect("vout fits"),
            spending_txid: spender.to_vec(),
        }
    }

    #[test]
    fn db11_an_unmined_spend_is_recorded_with_the_height_it_was_seen_at() {
        let Some(pool) = crate::db::shared_test_pool() else {
            eprintln!("skipping db11: UAAS_TEST_POSTGRES_URL not set");
            return;
        };
        let mut conn = pool.get().expect("fixture connection");

        let (txid, vout, spender) = spend_fixture(0x0db2_0001, 0x0db2_00a1);
        seed_live_utxo(&mut conn, &txid, vout);
        conn.execute("DELETE FROM utxo_unmined_spend WHERE txid = $1", &[&txid])
            .expect("clear fixture");

        let mut database = database_for(&pool);
        database.utxo_batch_spend(vec![spend_record(&txid, vout, &spender)], 900);

        assert_eq!(
            unmined_seen_height(&mut conn, &txid),
            Some(900),
            "the spend must be recorded against the height it was seen at"
        );
    }

    #[test]
    fn db12_reclaiming_returns_the_outpoint_to_the_spendable_set() {
        let Some(pool) = crate::db::shared_test_pool() else {
            eprintln!("skipping db12: UAAS_TEST_POSTGRES_URL not set");
            return;
        };
        let mut conn = pool.get().expect("fixture connection");

        let (txid, vout, spender) = spend_fixture(0x0db2_0002, 0x0db2_00a2);
        seed_live_utxo(&mut conn, &txid, vout);
        conn.execute("DELETE FROM utxo_unmined_spend WHERE txid = $1", &[&txid])
            .expect("clear fixture");

        let mut database = database_for(&pool);
        database.utxo_batch_spend(vec![spend_record(&txid, vout, &spender)], 900);
        assert_eq!(live_count(&mut conn, &txid), 0, "spent, so not live");

        database.utxo_batch_reclaim(vec![outpoint_of(&txid, vout)]);

        assert_eq!(
            live_count(&mut conn, &txid),
            1,
            "the outpoint must be spendable again"
        );
        assert_eq!(
            settled_state(&mut conn, &txid),
            None,
            "and must no longer be recorded as a spend at all"
        );
        assert_eq!(
            unmined_seen_height(&mut conn, &txid),
            None,
            "and must no longer be a candidate for eviction"
        );

        // The row came back whole, not as a stub.
        let (satoshis, created): (i64, Option<i32>) = conn
            .query_one(
                "SELECT satoshis, created_height FROM utxo WHERE txid = $1",
                &[&txid],
            )
            .map(|row| (row.get(0), row.get(1)))
            .expect("read the reclaimed row");
        assert_eq!((satoshis, created), (1000, Some(300)));
    }

    #[test]
    fn db13_a_settle_clears_the_eviction_candidate() {
        let Some(pool) = crate::db::shared_test_pool() else {
            eprintln!("skipping db13: UAAS_TEST_POSTGRES_URL not set");
            return;
        };
        let mut conn = pool.get().expect("fixture connection");

        let (txid, vout, spender) = spend_fixture(0x0db2_0003, 0x0db2_00a3);
        seed_live_utxo(&mut conn, &txid, vout);
        conn.execute("DELETE FROM utxo_unmined_spend WHERE txid = $1", &[&txid])
            .expect("clear fixture");

        let mut database = database_for(&pool);
        database.utxo_batch_spend(vec![spend_record(&txid, vout, &spender)], 900);
        assert_eq!(unmined_seen_height(&mut conn, &txid), Some(900));

        database.utxo_batch_settle(vec![spend_record(&txid, vout, &spender)], 950);

        assert_eq!(
            unmined_seen_height(&mut conn, &txid),
            None,
            "a mined spend must stop being an eviction candidate"
        );
        assert_eq!(
            settled_state(&mut conn, &txid),
            Some((spender, Some(950))),
            "and must be settled"
        );
    }

    /// The malleation case, keyed on the outpoint rather than the txid.
    ///
    /// The spend is announced as A and mined as its malleated sibling B. If
    /// either statement keyed on the spending txid, the record left by A would
    /// survive the settle and the outpoint would later be reclaimed — handing
    /// back as spendable an output that a block has already spent.
    #[test]
    fn db14_a_spend_mined_under_a_malleated_txid_stops_being_a_candidate() {
        let Some(pool) = crate::db::shared_test_pool() else {
            eprintln!("skipping db14: UAAS_TEST_POSTGRES_URL not set");
            return;
        };
        let mut conn = pool.get().expect("fixture connection");

        let (txid, vout, announced) = spend_fixture(0x0db2_0004, 0x0db2_00a4);
        let mined = hash_of(0x0db2_00b4).0.to_vec();
        assert_ne!(announced, mined, "the fixture must be a malleated pair");

        seed_live_utxo(&mut conn, &txid, vout);
        conn.execute("DELETE FROM utxo_unmined_spend WHERE txid = $1", &[&txid])
            .expect("clear fixture");

        let mut database = database_for(&pool);
        // Seen in the mempool as A.
        database.utxo_batch_spend(vec![spend_record(&txid, vout, &announced)], 900);
        // Mined as B.
        database.utxo_batch_settle(vec![spend_record(&txid, vout, &mined)], 950);

        assert_eq!(
            unmined_seen_height(&mut conn, &txid),
            None,
            "the settle must clear the candidate left by the other txid"
        );
        assert_eq!(
            settled_state(&mut conn, &txid),
            Some((mined, Some(950))),
            "and record the txid the block actually carried"
        );
    }

    /// Ordering, which is what makes the reclaim safe to queue behind a settle.
    ///
    /// A reclaim is decided from a SELECT taken before the writer has applied
    /// everything queued ahead of it, so by the time it runs the spend may
    /// already have been mined. The `spent_height IS NULL` guard in the
    /// statement, not the caller's snapshot, is what must decide it.
    #[test]
    fn db15_a_reclaim_behind_a_settle_does_not_unspend_it() {
        let Some(pool) = crate::db::shared_test_pool() else {
            eprintln!("skipping db15: UAAS_TEST_POSTGRES_URL not set");
            return;
        };
        let mut conn = pool.get().expect("fixture connection");

        let (txid, vout, spender) = spend_fixture(0x0db2_0005, 0x0db2_00a5);
        seed_live_utxo(&mut conn, &txid, vout);
        conn.execute("DELETE FROM utxo_unmined_spend WHERE txid = $1", &[&txid])
            .expect("clear fixture");

        let mut database = database_for(&pool);
        database.utxo_batch_spend(vec![spend_record(&txid, vout, &spender)], 900);
        // The spend confirms while a reclaim for it is already in flight.
        database.utxo_batch_settle(vec![spend_record(&txid, vout, &spender)], 950);
        database.utxo_batch_reclaim(vec![outpoint_of(&txid, vout)]);

        assert_eq!(
            live_count(&mut conn, &txid),
            0,
            "a settled spend must not be handed back as spendable"
        );
        assert_eq!(
            settled_state(&mut conn, &txid),
            Some((spender, Some(950))),
            "and must keep the height it settled at"
        );
    }

    /// Idempotence the other way round: the spend confirms *after* the reclaim.
    #[test]
    fn db16_a_spend_confirming_after_a_reclaim_settles_normally() {
        let Some(pool) = crate::db::shared_test_pool() else {
            eprintln!("skipping db16: UAAS_TEST_POSTGRES_URL not set");
            return;
        };
        let mut conn = pool.get().expect("fixture connection");

        let (txid, vout, spender) = spend_fixture(0x0db2_0006, 0x0db2_00a6);
        seed_live_utxo(&mut conn, &txid, vout);
        conn.execute("DELETE FROM utxo_unmined_spend WHERE txid = $1", &[&txid])
            .expect("clear fixture");

        let mut database = database_for(&pool);
        database.utxo_batch_spend(vec![spend_record(&txid, vout, &spender)], 900);
        database.utxo_batch_reclaim(vec![outpoint_of(&txid, vout)]);
        assert_eq!(live_count(&mut conn, &txid), 1, "reclaimed");

        // The slow spend finally confirms.
        database.utxo_batch_settle(vec![spend_record(&txid, vout, &spender)], 1100);

        assert_eq!(
            live_count(&mut conn, &txid),
            0,
            "the late settle must take it out of the spendable set again"
        );
        assert_eq!(
            settled_state(&mut conn, &txid),
            Some((spender, Some(1100))),
            "settled at the height that mined it"
        );
        // And reclaiming again must be a no-op rather than resurrecting it.
        database.utxo_batch_reclaim(vec![outpoint_of(&txid, vout)]);
        assert_eq!(
            live_count(&mut conn, &txid),
            0,
            "a second reclaim must not undo the settle"
        );
    }

    /// Spending an outpoint that is not in the spendable set must record
    /// nothing at all.
    ///
    /// Written after getting it wrong: recording the sighting in its own
    /// statement inserted a row for every outpoint in the batch, including the
    /// ones the DELETE never found. Such a row names an outpoint with no
    /// `utxo_spent` row, so the eviction join never matches it and nothing ever
    /// deletes it — a permanent leak of exactly the kind CS-423 exists to
    /// remove, in the table added to remove it.
    ///
    /// Chaining off `spent` rather than `moved` is the stricter of the two
    /// correct-looking forms. They differ only when the DELETE finds a row and
    /// the INSERT then declines it, which needs an outpoint present in both
    /// `utxo` and `utxo_spent` at once; the schema invariant says that cannot
    /// happen, so this test does not reach that case.
    ///
    /// Chaining off  rather than  is the stricter of the two
    /// correct-looking forms: they differ only when the DELETE finds a row and
    /// the INSERT then declines it, which needs an outpoint present in both
    ///  and  at once. The schema invariant says that cannot
    /// happen, so this test does not reach that case.
    #[test]
    fn db17_spending_an_outpoint_that_is_not_live_records_nothing() {
        let Some(pool) = crate::db::shared_test_pool() else {
            eprintln!("skipping db17: UAAS_TEST_POSTGRES_URL not set");
            return;
        };
        let mut conn = pool.get().expect("fixture connection");

        let (txid, vout, spender) = spend_fixture(0x0db2_0007, 0x0db2_00a7);
        // Deliberately not seeded: the outpoint is in neither table.
        for table in ["utxo", "utxo_spent", "utxo_unmined_spend"] {
            conn.execute(&format!("DELETE FROM {table} WHERE txid = $1"), &[&txid])
                .expect("clear fixture");
        }

        let mut database = database_for(&pool);
        database.utxo_batch_spend(vec![spend_record(&txid, vout, &spender)], 900);

        assert_eq!(
            settled_state(&mut conn, &txid),
            None,
            "nothing was moved, so nothing must be recorded as spent"
        );
        assert_eq!(
            unmined_seen_height(&mut conn, &txid),
            None,
            "and nothing must be left behind as an eviction candidate"
        );
    }
}
