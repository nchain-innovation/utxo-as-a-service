use std::sync::mpsc;

use chain_gang::{messages::OutPoint, util::Hash256};

use mysql::{prelude::*, PooledConn, *};

use crate::config::Config;
use retry::{delay, retry};

// UtxoEntry - used to store data into utxo table
#[derive(Clone)]
pub struct UtxoEntryDB {
    pub hash: String,
    pub pos: u32,
    pub satoshis: i64,
    pub height: i32,
    pub pubkeyhash: String,
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
    pub age: u64,
    pub tx: String,
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

    fn utxo_batch_write(&mut self, utxo_entries: Vec<UtxoEntryDB>) {
        if utxo_entries.is_empty() {
            return;
        }
        // bulk/batch write tx output to utxo table

        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || {
                self.conn
            .exec_batch(
                //"INSERT OVERWRITE utxo (hash, pos, satoshis, height) VALUES (:hash, :pos, :satoshis, :height);",
                "REPLACE INTO utxo (hash, pos, satoshis, height, pubkeyhash) VALUES (:hash, :pos, :satoshis, :height, :pubkeyhash);",
                utxo_entries
                    .iter()
                    .map(|x| params! {
                        "hash" => x.hash.as_str(), "pos" => x.pos, "satoshis" => x.satoshis, "height" => x.height, "pubkeyhash" => x.pubkeyhash.as_str()}),
                )
            },
        );
        if let Err(err) = result {
            Self::log_write_error("utxo batch write", err);
        }
    }

    fn utxo_batch_delete(&mut self, utxo_deletes: Vec<OutPoint>) {
        if utxo_deletes.is_empty() {
            return;
        }
        // bulk/batch delete utxo table entries
        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || {
                self.conn.exec_batch(
                    "DELETE FROM utxo WHERE hash = :hash AND pos = :pos;",
                    utxo_deletes
                        .iter()
                        .map(|x| params! {"hash" => x.hash.encode(), "pos" => x.index}),
                )
            },
        );
        if let Err(err) = result {
            Self::log_write_error("utxo batch delete", err);
        }
    }

    fn tx_batch_write(&mut self, tx_entries: Vec<TxEntryWriteDB>) {
        if tx_entries.is_empty() {
            return;
        }
        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || {
                self.conn
                .exec_batch(
                    "INSERT INTO tx (hash, height, blockindex, txsize, satoshis) VALUES (:hash, :height, :blockindex, :txsize, :satoshis)",
                    tx_entries.iter().map(
                        |tx| params! {"hash" => tx.hash.encode(), "height" => tx.height, "blockindex"=> tx.blockindex, "txsize"=> tx.size, "satoshis" => tx.satoshis},
                    ),
                )
            },
        );
        if let Err(err) = result {
            Self::log_write_error("tx batch write", err);
        }
    }

    fn mempool_batch_write(&mut self, mempool_entries: Vec<MempoolEntryDB>) {
        if mempool_entries.is_empty() {
            return;
        }

        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || {
                self.conn.exec_batch(
                    "INSERT INTO mempool (hash, locktime, fee, time, tx) \
                     VALUES (:hash, :locktime, :fee, :time, :tx)",
                    mempool_entries.iter().map(|entry| {
                        params! {
                            "hash" => entry.hash.encode(),
                            "locktime" => entry.locktime,
                            "fee" => entry.fee,
                            "time" => entry.age,
                            "tx" => entry.tx.as_str(),
                        }
                    }),
                )
            },
        );
        if let Err(err) = result {
            Self::log_write_error("mempool batch write", err);
        }
    }

    fn mempool_batch_delete(&mut self, mempool_hashes: Vec<Hash256>) {
        if mempool_hashes.is_empty() {
            return;
        }
        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || {
                self.conn.exec_batch(
                    "DELETE FROM mempool WHERE hash = :hash;",
                    mempool_hashes
                        .iter()
                        .map(|x| params! {"hash" => x.encode()}),
                )
            },
        );
        if let Err(err) = result {
            Self::log_write_error("mempool batch delete", err);
        }
    }

    fn block_header_write(&mut self, block_header: BlockHeaderWriteDB) {
        let height = block_header.height;
        let hash = block_header.hash.encode();
        let version = block_header.version;
        let prev_hash = block_header.prev_hash.encode();
        let merkle_root = block_header.merkle_root.encode();
        let timestamp = block_header.timestamp;
        let bits = block_header.bits;
        let nonce = block_header.nonce;
        let position = block_header.position;
        let blocksize = block_header.blocksize;
        let numtxs = block_header.numtxs;

        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || {
                self.conn.exec_drop(
                    r"INSERT INTO blocks
                    (height, hash, version, prev_hash, merkle_root, timestamp, bits, nonce, `offset`, blocksize, numtxs)
                    VALUES (:height, :hash, :version, :prev_hash, :merkle_root, :timestamp, :bits, :nonce, :offset, :blocksize, :numtxs)",
                    params! {
                        "height" => height,
                        "hash" => hash.as_str(),
                        "version" => version,
                        "prev_hash" => prev_hash.as_str(),
                        "merkle_root" => merkle_root.as_str(),
                        "timestamp" => timestamp,
                        "bits" => bits,
                        "nonce" => nonce,
                        "offset" => position,
                        "blocksize" => blocksize,
                        "numtxs" => numtxs,
                    },
                )
            },
        );
        if let Err(err) = result {
            Self::log_write_error("block header write", err);
        }
    }

    fn block_header_delete(&mut self, hash: &Hash256) {
        let hash = hash.encode();
        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || {
                self.conn.exec_drop(
                    "DELETE FROM blocks WHERE hash = :hash",
                    params! { "hash" => hash.as_str() },
                )
            },
        );
        if let Err(err) = result {
            Self::log_write_error("block header delete", err);
        }
    }

    fn tx_delete_at_height(&mut self, height: u32) {
        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || {
                self.conn.exec_drop(
                    "DELETE FROM tx WHERE height = :height",
                    params! { "height" => height },
                )
            },
        );
        if let Err(err) = result {
            Self::log_write_error("tx delete at height", err);
        }
    }

    fn utxo_delete_at_height(&mut self, height: u32) {
        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || {
                self.conn.exec_drop(
                    "DELETE FROM utxo WHERE height = :height",
                    params! { "height" => height },
                )
            },
        );
        if let Err(err) = result {
            Self::log_write_error("utxo delete at height", err);
        }
    }

    fn orphan_block_header_write(&mut self, block_header: OrphanBlockHeaderWriteDB) {
        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || {
                self.conn
                .exec_drop(
                r"INSERT INTO orphans (height, hash, version, prev_hash, merkle_root, timestamp, bits, nonce)
                VALUES (:height, :hash, :version, :prev_hash, :merkle_root, :timestamp, :bits, :nonce)",
                    params! {
                        "height" => block_header.height,
                        "hash" => block_header.hash.encode(),
                        "version" => block_header.version,
                        "prev_hash" => block_header.prev_hash.encode(),
                        "merkle_root" => block_header.merkle_root.encode(),
                        "timestamp"  => block_header.timestamp,
                        "bits"  => block_header.bits,
                        "nonce"  => block_header.nonce
                    })
            },
        );
        if let Err(err) = result {
            Self::log_write_error("orphan block header write", err);
        }
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
    //use mysql::Pool;

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
                    hash: hash_of(*id).encode(),
                    pos: *id,
                    satoshis: 1_000,
                    height: 1,
                    pubkeyhash: "unknown".to_string(),
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
                    tx: String::new(),
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
    #[test]
    fn db07_delete_between_writes_reaches_the_database() {
        let Ok(url) = std::env::var("UAAS_TEST_MYSQL_URL") else {
            eprintln!("skipping db07_delete_between_writes_reaches_the_database: UAAS_TEST_MYSQL_URL not set");
            return;
        };

        let pool = Pool::new(url.as_str()).expect("connect to UAAS_TEST_MYSQL_URL");
        let mut setup = pool
            .get_conn()
            .expect("get connection for utxo table setup");
        setup
            .query_drop(
                "CREATE TABLE IF NOT EXISTS utxo (
                    hash varchar(64) not null,
                    pos int unsigned not null,
                    satoshis bigint unsigned not null,
                    height int not null,
                    pubkeyhash varchar(64),
                    PRIMARY KEY (hash, pos)
                )",
            )
            .expect("create utxo table");

        // Ids well clear of the other tests' rows.
        let spent = 0xdb00_0001u32;
        let kept = 0xdb00_0002u32;
        for id in [spent, kept] {
            setup
                .exec_drop(
                    "DELETE FROM utxo WHERE hash = :hash",
                    params! { "hash" => hash_of(id).encode() },
                )
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
            conn: pool.get_conn().expect("get connection for writer"),
            rx,
            ms_delay: 300,
            retries: 3,
        };
        database.perform_db_operations();

        let present = |conn: &mut PooledConn, id: u32| -> bool {
            conn.exec_first::<u32, _, _>(
                "SELECT pos FROM utxo WHERE hash = :hash AND pos = :pos",
                params! { "hash" => hash_of(id).encode(), "pos" => id },
            )
            .expect("query utxo row")
            .is_some()
        };

        assert!(
            !present(&mut setup, spent),
            "the delete must have been applied, not dropped while coalescing the write"
        );
        assert!(
            present(&mut setup, kept),
            "the write after the delete must still have been applied"
        );

        for id in [spent, kept] {
            setup
                .exec_drop(
                    "DELETE FROM utxo WHERE hash = :hash",
                    params! { "hash" => hash_of(id).encode() },
                )
                .expect("clean up fixture rows");
        }
    }

    #[test]
    fn test_operation() {
        let Some(url) = std::env::var("UAAS_TEST_MYSQL_URL").ok() else {
            eprintln!("skipping database integration test: UAAS_TEST_MYSQL_URL not set");
            return;
        };

        let pool = Pool::new(url.as_str()).expect("connect to UAAS_TEST_MYSQL_URL");
        let conn = pool
            .get_conn()
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
