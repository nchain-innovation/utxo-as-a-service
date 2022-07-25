use std::sync::mpsc;

use sv::messages::OutPoint;
use sv::util::Hash256;

use mysql::prelude::*;
use mysql::PooledConn;
use mysql::*;

// UtxoEntry - used to store data into utxo table
#[derive(Clone)]
pub struct UtxoEntryDB {
    pub hash: String,
    pub pos: u32,
    pub satoshis: i64,
    pub height: i32,
}

// Used to store txs to write (in blocks)
#[derive(Clone)]
pub struct TxEntryWriteDB {
    pub hash: Hash256,
    pub height: usize,
    pub blockindex: u32,
    pub size: u32,
}

// database header structure
#[derive(Clone)]
pub struct DBBlockHeaderWrite {
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

// DBOperationType - used to identify the type of operation that the database needs to do
pub enum DBOperationType {
    UtxoBatchWrite(Vec<UtxoEntryDB>),
    UtxoBatchDelete(Vec<OutPoint>),
    TxBatchWrite(Vec<TxEntryWriteDB>),
    MempoolBatchDelete(Vec<Hash256>),
    BlockHeaderWrite(DBBlockHeaderWrite),
}

// This will be run in a separate thread that will be responsible for all the database writes
// so as not to delay the main thread of execution during IBD
pub struct Database {
    // Database connection
    conn: PooledConn,
    // Channel on which to receive operations
    rx: mpsc::Receiver<DBOperationType>,
}

/*
Caller should set up channel and pass rx to database
    tx: mpsc::Sender<DBOperationType>,
    let (tx, rx) = mpsc::channel();
    let db = Database::new(conn, rx);
*/

impl Database {
    pub fn new(conn: PooledConn, rx: mpsc::Receiver<DBOperationType>) -> Self {
        // Used to recieve database operations for processing
        Database { conn, rx }
    }

    fn utxo_batch_write(&mut self, utxo_entries: Vec<UtxoEntryDB>) {
        // bulk/batch write tx output to utxo table
        self.conn
        .exec_batch(
            //"INSERT OVERWRITE utxo (hash, pos, satoshis, height) VALUES (:hash, :pos, :satoshis, :height);",
            "REPLACE INTO utxo (hash, pos, satoshis, height) VALUES (:hash, :pos, :satoshis, :height);",
            utxo_entries
                .iter()
                .map(|x| params! {
                    "hash" => x.hash.as_str(), "pos" => x.pos, "satoshis" => x.satoshis, "height" => x.height
                }),
        )
        .unwrap();
    }

    fn utxo_batch_delete(&mut self, utxo_deletes: Vec<OutPoint>) {
        // bulk/batch delete utxo table entries
        self.conn
            .exec_batch(
                "DELETE FROM utxo WHERE hash = :hash AND pos = :pos;",
                utxo_deletes
                    .iter()
                    .map(|x| params! {"hash" => x.hash.encode(), "pos" => x.index}),
            )
            .unwrap();
    }

    fn tx_batch_write(&mut self, tx_entries: Vec<TxEntryWriteDB>) {
        self.conn
        .exec_batch(
            "INSERT INTO tx (hash, height, blockindex, txsize) VALUES (:hash, :height, :blockindex, :txsize)",
            tx_entries.iter().map(
                |tx| params! {"hash" => tx.hash.encode(), "height" => tx.height, "blockindex"=> tx.blockindex, "txsize"=> tx.size},
            ),
        )
        .unwrap();
    }

    fn mempool_batch_delete(&mut self, mempool_hashes: Vec<Hash256>) {
        self.conn
            .exec_batch(
                "DELETE FROM mempool WHERE hash = :hash;",
                mempool_hashes
                    .iter()
                    .map(|x| params! {"hash" => x.encode()}),
            )
            .unwrap();
    }

    fn block_header_write(&mut self, block_header: DBBlockHeaderWrite) {
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
        self.conn.exec_drop(&blocks_insert, Params::Empty).unwrap();
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
                DBOperationType::MempoolBatchDelete(mempool_hashes) => {
                    self.mempool_batch_delete(mempool_hashes)
                }
                DBOperationType::BlockHeaderWrite(block_header) => {
                    self.block_header_write(block_header)
                }
            }
        }
    }
}
