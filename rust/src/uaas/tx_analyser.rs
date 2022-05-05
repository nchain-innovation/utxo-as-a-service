use std::collections::HashMap;

use mysql::prelude::*;
use mysql::PooledConn;
use mysql::*;

use crate::config::Config;
use sv::messages::{Block, OutPoint, Tx, TxOut};
use sv::script::Script;
use sv::util::Hash256;

/*
    in - unlock_script - script sig
    out - lock_script - script public key

*/

// Used to store the unspent txs
pub struct UnspentEntry {
    satoshis: i64,
    lock_script: Script,
    height: usize,
}

// Used to store all txs (in blocks)
pub struct TxEntry {
    tx: Tx,
    height: usize,
}

pub struct TxAnalyser {
    // All transactions
    txs: HashMap<Hash256, TxEntry>,

    // mempool - transactions that are not in blocks
    mempool: HashMap<Hash256, Tx>,

    // Unspent tx
    unspent: HashMap<OutPoint, UnspentEntry>,
    // database connection
    conn: PooledConn,
    // Address to script & tx mapping - replaced by collections
    // p2pkh_scripts: HashMap<String, P2PKH_Entry>,
}

impl TxAnalyser {
    pub fn new(_config: &Config, conn: PooledConn) -> Self {
        TxAnalyser {
            txs: HashMap::new(),
            mempool: HashMap::new(),
            unspent: HashMap::new(),
            conn,
        }
    }

    pub fn create_table(&mut self) {
        // Create tables, if required
        // Check for the tables
        let tables: Vec<String> = self
            .conn
            .query(
                "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_TYPE = 'BASE TABLE';",
            )
            .unwrap();

        if tables.iter().find(|x| x.as_str() == "tx").is_none() {
            self.conn
                .query_drop(r"CREATE TABLE tx (hash text, height int)")
                .unwrap();
        }

        if tables.iter().find(|x| x.as_str() == "mempool").is_none() {
            self.conn
                .query_drop(r"CREATE TABLE mempool (hash text)")
                .unwrap();
        }
    }

    fn is_spendable(&self, vout: &TxOut) -> bool {
        // Return true if the transaction output is spendable,
        // and therefore should go in the unspent outputs (UTXO) set.
        // OP_FALSE OP_RETURN (0x00, 0x61) is known to be unspendable.
        vout.lock_script.0[0..2] != vec![0x00, 0x6a]
    }

    pub fn process_block_tx(&mut self, tx: &Tx, height: usize, tx_index: usize) {
        // Process tx as received in a block
        let hash = tx.hash();

        // Store tx - note that we only do this for tx in a block
        let tx_entry = TxEntry {
            tx: tx.clone(),
            height,
        };

        if let Some(_prev) = self.txs.insert(hash, tx_entry) {
            // We must have already processed this tx in a block
            return;
        }
        // TODO write to database
        let tx_insert = format!("INSERT INTO tx VALUES ('{}', {});", &hash.encode(), &height);
        dbg!(&tx_insert);
        self.conn.exec_drop(&tx_insert, Params::Empty).unwrap();

        // Remove from mempool as now in block
        if let Some(_value) = self.mempool.remove(&hash) {
            // Remove from database
            let mempool_delete = format!("DELETE FROM mempool WHERE hash='{}';", &hash.encode());
            // dbg!(&mempool_delete);
            self.conn.exec_drop(&mempool_delete, Params::Empty).unwrap();
        }

        // Process inputs - remove from unspent
        if tx_index == 0 {
            // if is coinbase - nothing to process as these won't be in the unspent
        } else {
            for vin in tx.inputs.iter() {
                // Remove from unspent
                self.unspent.remove(&vin.prev_output);
            }
        }

        // Collection processing
        // TODO add here

        // Process outputs - add to unspent
        for (index, vout) in tx.outputs.iter().enumerate() {
            if self.is_spendable(vout) {
                let outpoint = OutPoint {
                    hash,
                    index: index.try_into().unwrap(),
                };

                let new_entry = UnspentEntry {
                    satoshis: vout.satoshis,
                    lock_script: vout.lock_script.clone(),
                    height,
                };
                self.unspent.insert(outpoint, new_entry);
            }
        }
    }

    pub fn process_block(&mut self, block: &Block, height: usize) {
        // Given a block process all the txs in it
        for (tx_index, tx) in block.txns.iter().enumerate() {
            self.process_block_tx(tx, height, tx_index);
        }
    }

    pub fn process_standalone_tx(&mut self, tx: &Tx) {
        // Process standalone tx as we receive them
        // standalone tx are txs that are not in a block
        let hash = tx.hash();
        // Add it to the mempool
        self.mempool.insert(hash, tx.clone());

        // Write mempool entry to database
        let mempool_insert = self
            .conn
            .prep("INSERT INTO mempool (hash) VALUES (:hash)")
            .unwrap();

        self.conn
            .exec_drop(&mempool_insert, params! { "hash" => hash.encode()  })
            .unwrap();
    }
}
