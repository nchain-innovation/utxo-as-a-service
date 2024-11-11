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
    MempoolWrite(MempoolEntryDB),
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

    fn utxo_batch_write(&mut self, utxo_entries: Vec<UtxoEntryDB>) {
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
        result.unwrap();
    }

    fn utxo_batch_delete(&mut self, utxo_deletes: Vec<OutPoint>) {
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
        result.unwrap();
    }

    fn tx_batch_write(&mut self, tx_entries: Vec<TxEntryWriteDB>) {
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
        result.unwrap();
    }

    fn mempool_write(&mut self, mempool_entry: MempoolEntryDB) {
        let mempool_insert = format!(
            "INSERT INTO mempool VALUES ('{}', {}, {}, {},'{}');",
            &mempool_entry.hash.encode(),
            &mempool_entry.locktime,
            &mempool_entry.fee,
            &mempool_entry.age,
            &mempool_entry.tx,
        );

        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || self.conn.exec_drop(&mempool_insert, Params::Empty),
        );
        result.unwrap();
        /*
        // Write mempool entry to database
        self.conn.exec_drop(&mempool_insert, Params::Empty).expect(
            "Problem writing to mempool table. Check that tx field is present in mempool table.\n",
        );
        */
    }

    fn mempool_batch_delete(&mut self, mempool_hashes: Vec<Hash256>) {
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
        result.unwrap();
    }

    fn block_header_write(&mut self, block_header: BlockHeaderWriteDB) {
        let blocks_insert = format!(
            "INSERT INTO blocks
            VALUES ({}, '{}', {}, '{}', '{}', {}, {}, {}, {}, {}, {});",
            block_header.height,
            block_header.hash.encode(),
            block_header.version,
            block_header.prev_hash.encode(),
            block_header.merkle_root.encode(),
            block_header.timestamp,
            block_header.bits,
            block_header.nonce,
            block_header.position,
            block_header.blocksize,
            block_header.numtxs,
        );
        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || self.conn.exec_drop(&blocks_insert, Params::Empty),
        );
        result.unwrap();
    }

    fn block_header_delete(&mut self, hash: &Hash256) {
        let block_delete = format!("DELETE FROM blocks WHERE hash = '{}';", hash.encode());
        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || self.conn.exec_drop(&block_delete, Params::Empty),
        );
        result.unwrap();
    }

    fn tx_delete_at_height(&mut self, height: u32) {
        let tx_delete = format!("DELETE FROM tx WHERE height = '{}';", height);
        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || self.conn.exec_drop(&tx_delete, Params::Empty),
        );
        result.unwrap();
    }

    fn utxo_delete_at_height(&mut self, height: u32) {
        let utxo_delete = format!("DELETE FROM utxo WHERE height = '{}';", height);
        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || self.conn.exec_drop(&utxo_delete, Params::Empty),
        );
        result.unwrap();
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
        result.unwrap();
    }

    pub fn perform_db_operations(&mut self) {
        while let Ok(op) = self.rx.recv() {
            match op {
                DBOperationType::UtxoBatchWrite(utxo_entries) => {
                    self.utxo_batch_write(utxo_entries)
                }
                DBOperationType::UtxoBatchDelete(utxo_deletes) => {
                    self.utxo_batch_delete(utxo_deletes)
                }
                DBOperationType::TxBatchWrite(tx_entries) => self.tx_batch_write(tx_entries),

                DBOperationType::MempoolWrite(mempool_entry) => self.mempool_write(mempool_entry),
                DBOperationType::MempoolBatchDelete(mempool_hashes) => {
                    self.mempool_batch_delete(mempool_hashes)
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
        let pool = Pool::new("mysql://maas:maas-password@localhost:3306/main_uaas_db")
            .expect("Problem connecting to database. Check database is connected and database connection configuration is correct.\n");
        let conn = pool.get_conn().unwrap();
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
