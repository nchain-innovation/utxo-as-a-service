use std::collections::HashMap;

use mysql::prelude::*;
use mysql::PooledConn;
use mysql::*;

use std::time::{SystemTime, UNIX_EPOCH};

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

// Used to store all txs (in mempool)
pub struct MempoolEntry {
    tx: Tx,
    age: u64,
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
    mempool: HashMap<Hash256, MempoolEntry>,

    // Unspent tx
    unspent: HashMap<OutPoint, UnspentEntry>,
    // Database connection
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

    fn create_tables(&mut self) {
        // Create tables, if required
        // Check for the tables
        let tables: Vec<String> = self
            .conn
            .query(
                "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_TYPE = 'BASE TABLE';",
            )
            .unwrap();

        if !tables.iter().any(|x| x.as_str() == "tx") {
            self.conn
                .query_drop(
                    r"CREATE TABLE tx (
                    hash text,
                    height int unsigned);",
                )
                .unwrap();
        }

        if !tables.iter().any(|x| x.as_str() == "mempool") {
            self.conn
                .query_drop(
                    r"CREATE TABLE mempool (
                    hash text,
                    time int unsigned)",
                )
                .unwrap();
        }
    }

    pub fn setup(&mut self) {
        // Do the startup setup that is required for tx analyser
        self.create_tables();
    }

    fn is_spendable(&self, vout: &TxOut) -> bool {
        // Return true if the transaction output is spendable,
        // and therefore should go in the unspent outputs (UTXO) set.
        // OP_FALSE OP_RETURN (0x00, 0x61) is known to be unspendable.
        // dbg!(&vout.lock_script.0);
        // dbg!(&vout.lock_script.0[0..2]);
        if vout.lock_script.0.len() < 2 {
            false
        } else {
            vout.lock_script.0[0..2] != vec![0x00, 0x6a]
        }
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
            assert_eq!(1, 2); // should not get here!
            return;
        }

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
        /*
        dbg!(&tx);
        dbg!(&tx.hash());
        */
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
        // write txs to database
        let hashes: Vec<String> = block.txns.iter().map(|b| b.hash().encode()).collect();
        //  .query_drop(r"CREATE TABLE tx (hash text, height int)")

        // TODO write to database tx table
        //let tx_insert = format!("INSERT INTO tx (hash, height) VALUES (:hash, :height)", &hash.encode(), &height);
        self.conn
            .exec_batch(
                "INSERT INTO tx (hash, height) VALUES (:hash, :height)",
                hashes
                    .iter()
                    .map(|h| params! {"hash" => h, "height" => height }),
            )
            .unwrap();
    }

    pub fn process_standalone_tx(&mut self, tx: &Tx) {
        // Process standalone tx as we receive them.
        // Note standalone tx are txs that are not in a block.
        let hash = tx.hash();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Add it to the mempool
        let mempool_entry = MempoolEntry {
            tx: tx.clone(),
            age: now,
        };
        self.mempool.insert(hash, mempool_entry);

        // Write mempool entry to database
        let mempool_insert = format!(
            "INSERT INTO mempool VALUES ('{}', {});",
            &hash.encode(),
            &now
        );
        dbg!(&mempool_insert);
        self.conn.exec_drop(&mempool_insert, Params::Empty).unwrap();
    }
}
