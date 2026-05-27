use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chain_gang::messages::{Block, Payload, Tx};
use chain_gang::util::{Hash256, Serializable};

use super::hexslice::HexSlice;

use mysql::prelude::*;
use mysql::PooledConn;

use super::database::{DBOperationType, MempoolEntryDB, TxEntryWriteDB};

// Used for loading tx from mempool table
pub struct MempoolEntryReadDB {
    _hash: String,
}

// Used for loading tx from tx table
struct TxEntryDB {
    hash: String,
    height: u32,
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

    // Channel to database
    tx: mpsc::Sender<DBOperationType>,
}

impl TxDB {
    fn send_db_op(&self, op: DBOperationType) {
        if self.tx.send(op).is_err() {
            log::error!("Failed to send tx database operation; channel closed");
        }
    }

    fn decode_stored_hash(value: &str) -> Option<Hash256> {
        match Hash256::decode(value) {
            Ok(hash) => Some(hash),
            Err(err) => {
                log::error!("Invalid stored tx hash {value}: {err:?}");
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
            tx,
        }
    }

    pub fn create_tx_table(&mut self) {
        // Create tx table
        log::info!("Table tx not found - creating");
        if let Err(err) = self.conn.query_drop(
            r"CREATE TABLE tx (
                hash varchar(64) not null,
                height int unsigned not null,
                blockindex int unsigned not null,
                txsize int unsigned not null,
                satoshis bigint unsigned not null,
                CONSTRAINT PK_Entry PRIMARY KEY (hash));",
        ) {
            log::error!("Unable to create tx table: {err:?}");
            return;
        }

        if let Err(err) = self.conn.query_drop(
            r"CREATE INDEX IF NOT EXISTS idx_tx_height_blockindex ON tx (height, blockindex);",
        ) {
            log::error!("Unable to create tx height index: {err:?}");
        }
    }

    pub fn create_mempool_table(&mut self) {
        log::info!("Table mempool not found - creating");
        if let Err(err) = self.conn.query_drop(
            r"CREATE TABLE mempool (
                hash varchar(64) not null,
                locktime int unsigned not null,
                fee bigint unsigned not null,
                time int unsigned not null,
                tx longtext not null,
                CONSTRAINT PK_Mempool PRIMARY KEY (hash))",
        ) {
            log::error!("Unable to create mempool table: {err:?}");
        }
        // Note that tx longtext should be good for 4GB txs
    }

    pub fn load_tx(&mut self) {
        // Load tx - (tx hash and height) from database
        let start = Instant::now();

        let txs: Vec<TxEntryDB> = match self.conn.query_map(
            "SELECT hash, height FROM tx ORDER BY height",
            |(hash, height)| TxEntryDB { hash, height },
        ) {
            Ok(txs) => txs,
            Err(err) => {
                log::error!("Unable to load txs from database: {err:?}");
                return;
            }
        };

        for tx in txs {
            let Some(hash) = Self::decode_stored_hash(&tx.hash) else {
                continue;
            };
            self.txs.insert(hash, tx.height);
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

        let txs: Vec<MempoolEntryReadDB> = match self
            .conn
            .query_map("SELECT hash FROM mempool ORDER BY time", |hash| {
                MempoolEntryReadDB { _hash: hash }
            }) {
            Ok(txs) => txs,
            Err(err) => {
                log::error!("Unable to load mempool from database: {err:?}");
                return;
            }
        };

        for tx in txs {
            let Some(hash) = Self::decode_stored_hash(&tx._hash) else {
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
                    log::warn!("Should not get here, as it indicates that we have processed the same tx twice in a block. {:?}", &hash);
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
        let tx_hex = format!("{}", HexSlice::new(&b));

        let mempool_entry = MempoolEntryDB {
            hash,
            locktime: tx.lock_time,
            fee,
            age,
            tx: tx_hex,
        };

        self.send_db_op(DBOperationType::MempoolWrite(mempool_entry));
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
