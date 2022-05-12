use std::cmp;
use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use mysql::prelude::*;
use mysql::PooledConn;
use mysql::*;

use sv::messages::{Block, OutPoint, Tx, TxOut};
use sv::util::Hash256;

use crate::config::Config;

/*
    in - unlock_script - script sig
    out - lock_script - script public key

*/

const NOT_IN_BLOCK: i32 = -1; // use -1 to indicate that this tx is not in block

// Used to store the unspent txs (UTXO)
pub struct UnspentEntry {
    satoshis: i64,
    // lock_script: Script, - have seen some very large script lengths here - removed for now
    height: i32, // use -1 to indicate that tx is not in block
}

// UtxoEntry - used to store data into utxo table
struct UtxoEntry {
    pub hash: String,
    pub pos: u32,
    pub satoshis: i64,
    pub height: i32,
}

// Used to store all txs (in mempool)
pub struct MempoolEntry {
    tx: Option<Tx>,
    locktime: u32,
    fee: u64,
    age: u64,
}

// Used for loading tx from mempool table
pub struct MempoolDB {
    pub hash: String,
    locktime: u32,
    fee: u64,
    age: u64,
}

// Used to store all txs (in blocks)
pub struct TxEntry {
    tx: Option<Tx>,
    height: usize,
}

// Used for loading tx from tx table
struct HashHeight {
    pub hash: String,
    pub height: usize,
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
            println!("Table tx not found - creating");
            self.conn
                .query_drop(
                    r"CREATE TABLE tx (
                    hash varchar(64),
                    height int unsigned);",
                )
                .unwrap();
        }

        if !tables.iter().any(|x| x.as_str() == "mempool") {
            println!("Table mempool not found - creating");
            self.conn
                .query_drop(
                    r"CREATE TABLE mempool (
                    hash varchar(64),
                    locktime int unsigned,
                    fee bigint unsigned,
                    time int unsigned)",
                )
                .unwrap();
        }

        // utxo
        if !tables.iter().any(|x| x.as_str() == "utxo") {
            println!("Table utxo not found - creating");
            self.conn
                .query_drop(
                    r"CREATE TABLE utxo (
                    hash varchar(64),
                    pos int unsigned,
                    satoshis bigint unsigned,
                    height int)",
                )
                .unwrap();

            // index
            self.conn
                .query_drop(r"CREATE INDEX hash_pos ON utxo (hash, pos);")
                .unwrap();
        }
    }

    fn load_tx(&mut self) {
        // load tx hash and height from database
        let start = Instant::now();

        let txs: Vec<HashHeight> = self
            .conn
            .query_map("SELECT * FROM tx ORDER BY height", |(hash, height)| {
                HashHeight { hash, height }
            })
            .unwrap();

        for tx in txs {
            let tx_entry = TxEntry {
                tx: None,
                height: tx.height,
            };
            let hash = Hash256::decode(&tx.hash).unwrap();
            self.txs.insert(hash, tx_entry);
        }
        println!(
            "Loaded {} txs in {} seconds",
            self.txs.len(),
            start.elapsed().as_millis() as f64 / 1000.0
        );
    }

    fn load_mempool(&mut self) {
        // load tx hash and height from database
        let start = Instant::now();

        let txs: Vec<MempoolDB> = self
            .conn
            .query_map(
                "SELECT * FROM mempool ORDER BY time",
                |(hash, locktime, fee, time)| MempoolDB {
                    hash,
                    locktime,
                    fee,
                    age: time,
                },
            )
            .unwrap();

        for tx in txs {
            let mempool_entry = MempoolEntry {
                tx: None,
                age: tx.age,
                locktime: tx.locktime,
                fee: tx.fee,
            };
            let hash = Hash256::decode(&tx.hash).unwrap();
            self.mempool.insert(hash, mempool_entry);
        }

        println!(
            "Loaded {} mempool in {} seconds",
            self.txs.len(),
            start.elapsed().as_millis() as f64 / 1000.0
        );
    }

    fn load_utxo(&mut self) {
        // load outpoints from database
        let start = Instant::now();

        let txs: Vec<UtxoEntry> = self
            .conn
            .query_map(
                "SELECT * FROM mempool ORDER BY time",
                |(hash, pos, satoshis, height)| UtxoEntry {
                    hash,
                    pos,
                    satoshis,
                    height,
                },
            )
            .unwrap();

        for tx in txs {
            let hash = Hash256::decode(&tx.hash).unwrap();

            let outpoint = OutPoint {
                hash,
                index: tx.pos,
            };
            let utxo_entry = UnspentEntry {
                satoshis: tx.satoshis,
                height: tx.height,
            };
            // add to list
            self.unspent.insert(outpoint, utxo_entry);
        }

        println!(
            "Loaded {} utxo in {} seconds",
            self.txs.len(),
            start.elapsed().as_millis() as f64 / 1000.0
        );
    }

    fn read_tables(&mut self) {
        // load tx - Note  - can't load the tx as we dont store the original tx
        self.load_tx();
        // load mempool
        self.load_mempool();
        // load utxo
        self.load_utxo();
    }

    pub fn setup(&mut self) {
        // Do the startup setup that is required for tx analyser
        self.create_tables();

        self.read_tables();
    }

    fn is_spendable(&self, vout: &TxOut) -> bool {
        // Return true if the transaction output is spendable,
        // and therefore should go in the unspent outputs (UTXO) set.
        // OP_FALSE OP_RETURN (0x00, 0x61) is known to be unspendable.

        if vout.lock_script.0.len() < 2 {
            // We are assuming that [] is spendable
            true
        } else {
            vout.lock_script.0[0..2] != vec![0x00, 0x6a]
        }
    }

    fn process_tx_outputs(&mut self, tx: &Tx, height: i32) {
        // process the tx outputs and place them in the unspents
        // Record for batch write to utxo table
        let mut utxo_entries: Vec<UtxoEntry> = Vec::new();

        let hash = tx.hash();

        // Process outputs - add to unspent
        for (index, vout) in tx.outputs.iter().enumerate() {
            if self.is_spendable(vout) {
                let outpoint = OutPoint {
                    hash,
                    index: index.try_into().unwrap(),
                };

                let new_entry = UnspentEntry {
                    satoshis: vout.satoshis,
                    // lock_script: vout.lock_script.clone(),
                    height,
                };
                // add to list
                self.unspent.insert(outpoint, new_entry);

                // Record for batch write to utxo table
                let utxo_entry = UtxoEntry {
                    hash: hash.encode(),
                    pos: index.try_into().unwrap(),
                    satoshis: vout.satoshis,
                    height,
                };
                utxo_entries.push(utxo_entry);
            }
        }
        // bulk/batch write tx output to utxo table
        self.conn
            .exec_batch(
                "INSERT INTO utxo (hash, pos, satoshis, height) VALUES (:hash, :pos, :satoshis, :height);",
                utxo_entries
                    .iter()
                    .map(|x| params! {
                        "hash" => x.hash.as_str(), "pos" => x.pos, "satoshis" => x.satoshis, "height" => x.height
                    }),
            )
            .unwrap();
    }

    pub fn process_block_tx(&mut self, tx: &Tx, height: i32, tx_index: usize) {
        // Process tx as received in a block
        let hash = tx.hash();

        // Store tx - note that we only do this for tx in a block
        let tx_entry = TxEntry {
            tx: Some(tx.clone()),
            height: height.try_into().unwrap(),
        };

        if let Some(_prev) = self.txs.insert(hash, tx_entry) {
            // We must have already processed this tx in a block
            panic!("Should not get here, as it indicates that we have processed the same tx twice in a block.");
        }

        // Remove from mempool as now in block
        if let Some(_value) = self.mempool.remove(&hash) {
            // Remove from database
            let mempool_delete = format!("DELETE FROM mempool WHERE hash='{}';", &hash.encode());
            self.conn.exec_drop(&mempool_delete, Params::Empty).unwrap();
        }

        // Process inputs - remove from unspent
        let mut utxo_deletes: Vec<&OutPoint> = Vec::new();

        if tx_index == 0 {
            // if is coinbase - nothing to process as these won't be in the unspent
        } else {
            for vin in tx.inputs.iter() {
                // Remove from unspent
                self.unspent.remove(&vin.prev_output);
                // Remove from utxo table
                utxo_deletes.push(&vin.prev_output);
            }
        }
        // bulk/batch delete utxo table entries
        self.conn
            .exec_batch(
                "DELETE FROM utxo WHERE hash = :hash AND pos = :pos;",
                utxo_deletes
                    .iter()
                    .map(|x| params! {"hash" => x.hash.encode(), "pos" => x.index}),
            )
            .unwrap();

        // Collection processing
        // TODO add here

        // Process outputs
        self.process_tx_outputs(tx, height);
    }

    pub fn process_block(&mut self, block: &Block, height: i32) {
        // Given a block process all the txs in it
        for (tx_index, tx) in block.txns.iter().enumerate() {
            self.process_block_tx(tx, height, tx_index);
        }
        // write txs to database
        let hashes: Vec<String> = block.txns.iter().map(|b| b.hash().encode()).collect();

        // Batch write tx to database table
        self.conn
            .exec_batch(
                "INSERT INTO tx (hash, height) VALUES (:hash, :height)",
                hashes
                    .iter()
                    .map(|h| params! {"hash" => h, "height" => height}),
            )
            .unwrap();
    }

    fn calc_fee(&self, tx: &Tx) -> i64 {
        // Given the tx attempt to determine the fee, return 0 if unable to calculate
        let mut inputs = 0i64;
        for vin in tx.inputs.iter() {
            if let Some(entry) = self.unspent.get(&vin.prev_output) {
                inputs += entry.satoshis;
            } else {
                // if any of the inputs are missing then return 0
                return 0;
            }
        }
        let outputs: i64 = tx.outputs.iter().map(|vout| vout.satoshis).sum();
        // Determine the difference between the inputs and the outputs
        let fee = inputs - outputs;
        //println!("fee={} ({} - {})", fee, inputs, outputs);
        // Don't return a negative fee, it must be at least 0
        cmp::max(0i64, fee)
    }

    pub fn process_standalone_tx(&mut self, tx: &Tx) {
        // Process standalone tx as we receive them.
        // Note standalone tx are txs that are not in a block.
        let hash = tx.hash();
        let age = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let locktime = tx.lock_time;
        let fee = self.calc_fee(tx);
        // Add it to the mempool
        let mempool_entry = MempoolEntry {
            tx: Some(tx.clone()),
            age,
            locktime,
            fee: fee.try_into().unwrap(),
        };
        self.mempool.insert(hash, mempool_entry);

        // Write mempool entry to database
        let mempool_insert = format!(
            "INSERT INTO mempool VALUES ('{}', {}, {}, {});",
            &hash.encode(),
            &locktime,
            &fee,
            &age,
        );
        self.conn.exec_drop(&mempool_insert, Params::Empty).unwrap();

        // Process outputs
        self.process_tx_outputs(tx, NOT_IN_BLOCK);
    }
}
