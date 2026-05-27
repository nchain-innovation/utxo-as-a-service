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

    fn coalesce_utxo_batch_write(&mut self, mut entries: Vec<UtxoEntryDB>) -> Vec<UtxoEntryDB> {
        while let Ok(DBOperationType::UtxoBatchWrite(more)) = self.rx.try_recv() {
            entries.extend(more);
        }
        entries
    }

    fn coalesce_utxo_batch_delete(&mut self, mut deletes: Vec<OutPoint>) -> Vec<OutPoint> {
        while let Ok(DBOperationType::UtxoBatchDelete(more)) = self.rx.try_recv() {
            deletes.extend(more);
        }
        deletes
    }

    fn coalesce_tx_batch_write(&mut self, mut entries: Vec<TxEntryWriteDB>) -> Vec<TxEntryWriteDB> {
        while let Ok(DBOperationType::TxBatchWrite(more)) = self.rx.try_recv() {
            entries.extend(more);
        }
        entries
    }

    fn coalesce_mempool_batch_write(
        &mut self,
        mut entries: Vec<MempoolEntryDB>,
    ) -> Vec<MempoolEntryDB> {
        while let Ok(DBOperationType::MempoolBatchWrite(more)) = self.rx.try_recv() {
            entries.extend(more);
        }
        entries
    }

    fn coalesce_mempool_batch_delete(&mut self, mut hashes: Vec<Hash256>) -> Vec<Hash256> {
        while let Ok(DBOperationType::MempoolBatchDelete(more)) = self.rx.try_recv() {
            hashes.extend(more);
        }
        hashes
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

    pub fn perform_db_operations(&mut self) {
        while let Ok(op) = self.rx.recv() {
            match op {
                DBOperationType::UtxoBatchWrite(entries) => {
                    let entries = self.coalesce_utxo_batch_write(entries);
                    self.utxo_batch_write(entries);
                }
                DBOperationType::UtxoBatchDelete(deletes) => {
                    let deletes = self.coalesce_utxo_batch_delete(deletes);
                    self.utxo_batch_delete(deletes);
                }
                DBOperationType::TxBatchWrite(entries) => {
                    let entries = self.coalesce_tx_batch_write(entries);
                    self.tx_batch_write(entries);
                }
                DBOperationType::MempoolBatchWrite(entries) => {
                    let entries = self.coalesce_mempool_batch_write(entries);
                    self.mempool_batch_write(entries);
                }
                DBOperationType::MempoolBatchDelete(hashes) => {
                    let hashes = self.coalesce_mempool_batch_delete(hashes);
                    self.mempool_batch_delete(hashes);
                }
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
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::mpsc;
    //use mysql::Pool;

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
