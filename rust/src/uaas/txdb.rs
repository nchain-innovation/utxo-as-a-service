use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chain_gang::messages::{Block, Payload, Tx};
use chain_gang::util::{Hash256, Serializable};

use crate::db::PooledConn;

use super::database::{DBOperationType, MempoolEntryDB, TxEntryWriteDB};

// Used for loading tx from mempool table
pub struct MempoolEntryReadDB {
    _hash: Vec<u8>,
}

// Used for loading tx from tx table
//
// `height` is the column's own type. `tx.height` is a signed `integer`, and
// the postgres crate maps Rust's `u32` to `oid`, not `int4` — decoding one
// straight into a `u32` panics. Narrowed after the read, like block_manager.
struct TxEntryDB {
    hash: Vec<u8>,
    height: i32,
}

// TxDB - wraps interface to tx and mempool database tables

pub struct TxDB {
    // Database connection
    conn: PooledConn,
    // All transactions
    pub txs: HashMap<Hash256, u32>,
    save_txs: bool,

    // mempool - transactions that are not in blocks
    pub mempool: HashMap<Hash256, Hash256>,

    // txs to remove from mempool table
    hashes_to_delete: Vec<Hash256>,

    // txs to add to tx table
    tx_entries: Vec<TxEntryWriteDB>,

    // mempool entries to write to database
    mempool_entries: Vec<MempoolEntryDB>,

    // Channel to database
    tx: mpsc::Sender<DBOperationType>,
}

impl TxDB {
    fn send_db_op(&self, op: DBOperationType) {
        if self.tx.send(op).is_err() {
            log::error!("Failed to send tx database operation; channel closed");
        }
    }

    /// Narrows a signed column to the unsigned value the rest of the service
    /// uses, dropping the row rather than wrapping if it does not fit.
    fn unsigned_from_column(label: &str, value: i32) -> Option<u32> {
        u32::try_from(value)
            .map_err(|_| {
                log::error!("Stored {label} is negative ({value}); row skipped");
            })
            .ok()
    }

    /// Stored hashes are raw `bytea` in internal order, not display-order hex.
    ///
    /// Both halves of that matter. The column is `bytea`, so reading it into a
    /// `String` does not fail — it *panics* inside `row.get`, which took the
    /// process down on every restart with a non-empty table (CS-429). And the
    /// write side binds `hash.0`, so even as text the order would have been
    /// reversed: `Hash256::decode` parses display order. Construct the hash
    /// from the bytes as stored, exactly as `utxo` and `collection` do.
    fn decode_stored_hash(label: &str, value: &[u8]) -> Option<Hash256> {
        match <[u8; 32]>::try_from(value) {
            Ok(bytes) => Some(Hash256(bytes)),
            Err(_) => {
                log::error!(
                    "Stored {label} hash is {} bytes, expected 32; row skipped",
                    value.len()
                );
                None
            }
        }
    }

    pub fn new(conn: PooledConn, tx: mpsc::Sender<DBOperationType>, save_txs: bool) -> Self {
        TxDB {
            conn,
            txs: HashMap::new(),
            save_txs,
            mempool: HashMap::new(),
            hashes_to_delete: Vec::new(),
            tx_entries: Vec::new(),
            mempool_entries: Vec::new(),
            tx,
        }
    }

    pub fn load_tx(&mut self) {
        // Load tx - (tx hash and height) from database
        let start = Instant::now();

        let txs: Vec<TxEntryDB> = match self
            .conn
            .query("SELECT hash, height FROM tx ORDER BY height", &[])
        {
            Ok(rows) => rows
                .iter()
                .map(|row| TxEntryDB {
                    hash: row.get(0),
                    height: row.get(1),
                })
                .collect(),
            Err(err) => {
                log::error!("Unable to load txs from database: {err:?}");
                return;
            }
        };

        for tx in txs {
            let Some(hash) = Self::decode_stored_hash("tx", &tx.hash) else {
                continue;
            };
            let Some(height) = Self::unsigned_from_column("tx height", tx.height) else {
                continue;
            };
            self.txs.insert(hash, height);
        }
        log::info!(
            "{} txs loaded in {} seconds",
            self.txs.len(),
            start.elapsed().as_millis() as f64 / 1000.0
        );
    }

    pub fn load_mempool(&mut self) {
        // load mempool - tx hash and height from database
        let start = Instant::now();

        // `time` became `seen_at`, a timestamptz rather than a unix integer.
        let txs: Vec<MempoolEntryReadDB> = match self
            .conn
            .query("SELECT hash FROM mempool ORDER BY seen_at", &[])
        {
            Ok(rows) => rows
                .iter()
                .map(|row| MempoolEntryReadDB { _hash: row.get(0) })
                .collect(),
            Err(err) => {
                log::error!("Unable to load mempool from database: {err:?}");
                return;
            }
        };

        for tx in txs {
            let Some(hash) = Self::decode_stored_hash("mempool", &tx._hash) else {
                continue;
            };
            self.mempool.insert(hash, hash);
        }

        log::info!(
            "{} Mempool tx Loaded in {} seconds",
            self.mempool.len(),
            start.elapsed().as_millis() as f64 / 1000.0
        );
    }

    // save the tx to the database
    fn save_tx(&mut self, tx: &Tx, hash: Hash256, blockindex: u32, height: usize) {
        let satoshi_sum: i64 = tx.outputs.iter().map(|x| x.satoshis).sum();
        let satoshi_out: u64 = match satoshi_sum.try_into() {
            Ok(value) => value,
            Err(_) => {
                log::warn!("Skipping tx {hash:?}: output satoshi sum {satoshi_sum} out of range");
                return;
            }
        };
        // Store tx - note that we only do this for tx in a block
        let tx_entry = TxEntryWriteDB {
            hash,
            height,
            blockindex,
            size: tx.size() as u32,
            satoshis: satoshi_out,
        };

        // Write to database later
        self.tx_entries.push(tx_entry);
    }

    pub fn process_block(&mut self, block: &Block, height: i32) {
        let height_usize = match height.try_into() {
            Ok(value) => value,
            Err(_) => {
                log::error!("Block height {height} out of range while processing txs");
                return;
            }
        };
        let height_u32 = match height.try_into() {
            Ok(value) => value,
            Err(_) => {
                log::error!("Block height {height} out of range while indexing txs");
                return;
            }
        };

        // for each tx in block
        for (blockindex, tx) in block.txns.iter().enumerate() {
            let hash = tx.hash();

            // if in mempool - remove and append to list of hashes to delete
            if self.mempool.remove(&hash).is_some() {
                self.hashes_to_delete.push(hash);
            }

            if self.save_txs {
                let blockindex_u32 = match blockindex.try_into() {
                    Ok(value) => value,
                    Err(_) => {
                        log::error!("Block index {blockindex} out of range for tx {hash:?}");
                        continue;
                    }
                };
                self.save_tx(tx, hash, blockindex_u32, height_usize);
                if self.txs.insert(hash, height_u32).is_some() {
                    // We must have already processed this tx in a block
                    log::warn!("Should not get here, as it indicates that we have processed the same tx twice in a block. {:?}", hash);
                }
            }
        }
    }

    pub fn batch_delete_from_mempool(&mut self) {
        // Batch Delete from mempool
        self.send_db_op(DBOperationType::MempoolBatchDelete(
            self.hashes_to_delete.clone(),
        ));
        self.hashes_to_delete.clear();
    }

    pub fn batch_write_tx_to_table(&mut self) {
        self.send_db_op(DBOperationType::TxBatchWrite(self.tx_entries.clone()));
        self.tx_entries.clear();
    }

    pub fn batch_write_mempool(&mut self) {
        if self.mempool_entries.is_empty() {
            return;
        }
        let entries = std::mem::take(&mut self.mempool_entries);
        self.send_db_op(DBOperationType::MempoolBatchWrite(entries));
    }

    pub fn add_to_mempool(&mut self, tx: &Tx, fee: i64) {
        let hash = tx.hash();
        let age = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_else(|err| {
                log::warn!("Unable to read system time for mempool entry: {err:?}");
                0
            });

        // Add it to the mempool
        self.mempool.insert(hash, hash);

        // Write the tx as hexstr
        let mut b = Vec::with_capacity(tx.size());
        if let Err(err) = tx.write(&mut b) {
            log::error!("Unable to serialize mempool tx {hash:?}: {err:?}");
            self.mempool.remove(&hash);
            return;
        }
        // Raw bytes: the column is `bytea`, not hex in a `longtext`.
        let mempool_entry = MempoolEntryDB {
            hash,
            locktime: tx.lock_time,
            fee,
            age,
            tx: b,
        };

        self.mempool_entries.push(mempool_entry);
    }

    pub fn tx_exists(&self, hash: Hash256) -> bool {
        // Return true if txid is in txs or mempool
        self.txs.contains_key(&hash) || self.mempool.contains_key(&hash)
    }

    pub fn handle_orphan_block(&mut self, height: u32) {
        // Remove transactions of this block height
        self.send_db_op(DBOperationType::TxDelete(height));

        // Remove transactions at this height
        self.txs.retain(|_hash, tx_height| *tx_height != height);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uaas::database::Database;

    /// A txid as the REST API states it, and the same txid as the database
    /// stores it. The same pair as `rust/tests/hash_order.rs`, deliberately:
    /// this module is the place the order was got wrong.
    const DISPLAY: &str = "00000000000000000545267003727771023c9822756f187cbee83a5329ffecd8";
    const STORED: &str = "d8ecff29533ae8be7c186f7522983c0271777203702645050000000000000000";

    #[test]
    fn txdb01_a_stored_hash_is_read_as_written_not_reversed() {
        let written = Hash256::decode(DISPLAY).expect("a valid txid decodes");
        // What the write side binds.
        let stored: Vec<u8> = written.0.to_vec();
        assert_eq!(
            hex::encode(&stored),
            STORED,
            "fixture pins the stored order"
        );

        let read = TxDB::decode_stored_hash("test", &stored).expect("32 bytes decode");
        assert_eq!(read, written);
        // The reversal that `Hash256::decode` would have applied must not have
        // happened. Without this the test passes on a palindrome.
        assert_ne!(hex::encode(read.0), DISPLAY);
    }

    #[test]
    fn txdb02_a_hash_of_the_wrong_length_is_skipped_not_a_panic() {
        assert!(TxDB::decode_stored_hash("test", &[0u8; 31]).is_none());
        assert!(TxDB::decode_stored_hash("test", &[0u8; 33]).is_none());
        assert!(TxDB::decode_stored_hash("test", &[]).is_none());
        assert!(TxDB::decode_stored_hash("test", &[0u8; 32]).is_some());
    }

    /// Distinct from every other fixture id in the suite.
    fn fixture_hash(tag: u8) -> Hash256 {
        let mut bytes = [0u8; 32];
        bytes[0] = 0xdb;
        bytes[1] = 0x0c;
        bytes[31] = tag;
        Hash256(bytes)
    }

    /// Writes through the real writer loop, then reads through the real load
    /// path. This is the round trip that the type mismatch broke: `row.get`
    /// panicked rather than erroring, so nothing short of executing both halves
    /// against a server can see it.
    ///
    /// Needs PostgreSQL with the schema applied, and skips rather than fails
    /// without one — same contract as `db07`.
    fn round_trip(ops: Vec<DBOperationType>, load: impl FnOnce(&mut TxDB)) -> Option<TxDB> {
        let Ok(url) = std::env::var("UAAS_TEST_POSTGRES_URL") else {
            eprintln!("skipping txdb round trip: UAAS_TEST_POSTGRES_URL not set");
            return None;
        };
        let pool = crate::db::build_pool(&url).expect("connect to UAAS_TEST_POSTGRES_URL");

        let (tx, rx) = mpsc::channel();
        for op in ops {
            tx.send(op).expect("send operation");
        }
        drop(tx);

        let config = crate::config::tests::sample_config();
        let mut database = Database::new(pool.get().expect("connection for writer"), rx, &config);
        database.perform_db_operations();

        let (unused_tx, _unused_rx) = mpsc::channel();
        let mut txdb = TxDB::new(pool.get().expect("connection for reader"), unused_tx, true);
        load(&mut txdb);
        Some(txdb)
    }

    fn clear(table: &str, hash: Hash256) {
        let Ok(url) = std::env::var("UAAS_TEST_POSTGRES_URL") else {
            return;
        };
        let pool = crate::db::build_pool(&url).expect("connect to UAAS_TEST_POSTGRES_URL");
        let mut conn = pool.get().expect("connection for fixture cleanup");
        conn.execute(
            &format!("DELETE FROM {table} WHERE hash = $1"),
            &[&&hash.0[..]],
        )
        .expect("clear fixture row");
    }

    #[test]
    fn txdb03_a_mempool_row_is_read_back_as_the_txid_that_was_written() {
        let hash = fixture_hash(0x01);
        clear("mempool", hash);

        let entry = MempoolEntryDB {
            hash,
            locktime: 0,
            fee: 7,
            age: 1_700_000_000,
            tx: vec![0xde, 0xad, 0xbe, 0xef],
        };
        let Some(txdb) = round_trip(
            vec![DBOperationType::MempoolBatchWrite(vec![entry])],
            |txdb| txdb.load_mempool(),
        ) else {
            return;
        };

        assert!(
            txdb.mempool.contains_key(&hash),
            "the mempool row written by the writer must load back as the same txid"
        );
        clear("mempool", hash);
    }

    #[test]
    fn txdb04_a_tx_row_is_read_back_as_the_txid_that_was_written() {
        let hash = fixture_hash(0x02);
        clear("tx", hash);

        let entry = TxEntryWriteDB {
            hash,
            height: 4242,
            blockindex: 1,
            size: 200,
            satoshis: 1000,
        };
        let Some(txdb) = round_trip(vec![DBOperationType::TxBatchWrite(vec![entry])], |txdb| {
            txdb.load_tx()
        }) else {
            return;
        };

        assert_eq!(
            txdb.txs.get(&hash),
            Some(&4242),
            "the tx row written by the writer must load back as the same txid"
        );
        clear("tx", hash);
    }
}
