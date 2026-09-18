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
    /// The bytes identifying what selected this output. Populated from
    /// `script_to_pubkeyhash` for now; CS-421 replaces it with the matching
    /// pattern's named capture.
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
pub enum DBOperationType {
    UtxoBatchWrite(Vec<UtxoEntryDB>),
    UtxoBatchDelete(Vec<OutPoint>),
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
                // locking_script is NOT NULL in the schema but the in-memory
                // UTXO set does not keep it — it was dropped from UtxoEntry
                // long ago because some scripts are very large. An empty script
                // is the honest placeholder until CS-421, which records the
                // script it matched against.
                let script: &[u8] = &[];
                tx.execute(
                    &stmt,
                    &[
                        &entry.hash,
                        &vout,
                        &entry.satoshis,
                        &script,
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
                "INSERT INTO mempool (hash, locktime, fee, seen_at, tx) \
                 VALUES ($1, $2, $3, $4, $5) \
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
        let Ok(url) = std::env::var("UAAS_TEST_POSTGRES_URL") else {
            eprintln!(
                "skipping db07_delete_between_writes_reaches_the_database: \
                 UAAS_TEST_POSTGRES_URL not set"
            );
            return;
        };

        let pool = crate::db::build_pool(&url).expect("connect to UAAS_TEST_POSTGRES_URL");
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

    #[test]
    fn test_operation() {
        let Some(url) = std::env::var("UAAS_TEST_POSTGRES_URL").ok() else {
            eprintln!("skipping database integration test: UAAS_TEST_POSTGRES_URL not set");
            return;
        };

        let pool = crate::db::build_pool(&url).expect("connect to UAAS_TEST_POSTGRES_URL");
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
}
