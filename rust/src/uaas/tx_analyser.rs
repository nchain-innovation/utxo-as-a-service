use std::cmp;
use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use mysql::prelude::*;
use mysql::PooledConn;
use mysql::*;

use sv::messages::{Block, OutPoint, Payload, Tx, TxOut};
use sv::util::Hash256;

use crate::config::Config;
use crate::uaas::collection::WorkingCollection;
/*
    in - unlock_script - script sig
    out - lock_script - script public key

*/

const NOT_IN_BLOCK: i32 = -1; // use -1 to indicate that this tx is not in block

// Used to store the unspent txs (UTXO)
pub struct UnspentEntry {
    satoshis: i64,
    // lock_script: Script, - have seen some very large script lengths here - removed for now
    _height: i32, // use NOT_IN_BLOCK -1 to indicate that tx is not in block
}

// UtxoEntry - used to store data into utxo table
#[derive(Debug)]

struct UtxoEntry {
    pub hash: String,
    pub pos: u32,
    pub satoshis: i64,
    pub height: i32,
}

// Used to store all txs (in mempool)
pub struct MempoolEntry {
    _tx: Option<Tx>,
    _locktime: u32,
    _fee: u64,
    _age: u64,
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
    _tx: Option<Tx>,
    _height: usize,
    _size: u32,
}

// Used for loading tx from tx table
struct HashHeight {
    pub hash: String,
    pub height: usize,
    pub size: u32,
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

    // Collections
    collection: Vec<WorkingCollection>,
}

impl TxAnalyser {
    pub fn new(config: &Config, conn: PooledConn) -> Self {
        let mut txanal = TxAnalyser {
            txs: HashMap::new(),
            mempool: HashMap::new(),
            unspent: HashMap::new(),
            conn,
            collection: Vec::new(),
        };

        for collection in &config.collection {
            let wc = WorkingCollection::new(collection.clone());
            txanal.collection.push(wc);
        }
        txanal
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
                    height int unsigned,
                    txsize int unsigned,
                    CONSTRAINT PK_Entry PRIMARY KEY (hash));",
                )
                .unwrap();
            self.conn
                .query_drop(r"CREATE INDEX idx_tx ON tx (hash);")
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
            self.conn
                .query_drop(r"CREATE INDEX idx_txkey ON mempool (hash);")
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
                    height int,
                    CONSTRAINT PK_Entry PRIMARY KEY (hash, pos));",
                )
                .unwrap();
            self.conn
                .query_drop(r"CREATE INDEX idx_key ON utxo (hash, pos);")
                .unwrap();
        }

        // Collection tables
        for c in &self.collection {
            let name = c.name();
            if !tables.iter().any(|x| x.as_str() == name) {
                println!("Table collection {} not found - creating", name);
                c.create_table(&mut self.conn);
            }
        }
    }

    fn load_tx(&mut self) {
        // load tx - (tx hash and height) from database
        let start = Instant::now();

        let txs: Vec<HashHeight> = self
            .conn
            .query_map(
                "SELECT * FROM tx ORDER BY height",
                |(hash, height, size)| HashHeight { hash, height, size },
            )
            .unwrap();

        for tx in txs {
            let tx_entry = TxEntry {
                _tx: None,
                _height: tx.height,
                _size: tx.size,
            };
            let hash = Hash256::decode(&tx.hash).unwrap();
            self.txs.insert(hash, tx_entry);
        }
        println!(
            "Txs {} loaded in {} seconds",
            self.txs.len(),
            start.elapsed().as_millis() as f64 / 1000.0
        );
    }

    fn load_mempool(&mut self) {
        // load mempool - tx hash and height from database
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
                _tx: None,
                _age: tx.age,
                _locktime: tx.locktime,
                _fee: tx.fee,
            };
            let hash = Hash256::decode(&tx.hash).unwrap();
            self.mempool.insert(hash, mempool_entry);
        }

        println!(
            "Mempool {} Loaded in {} seconds",
            self.mempool.len(),
            start.elapsed().as_millis() as f64 / 1000.0
        );
    }

    fn load_utxo(&mut self) {
        // load outpoints from database
        let start = Instant::now();

        let txs: Vec<UtxoEntry> = self
            .conn
            .query_map("SELECT * FROM utxo", |(hash, pos, satoshis, height)| {
                UtxoEntry {
                    hash,
                    pos,
                    satoshis,
                    height,
                }
            })
            .unwrap();

        for tx in txs {
            let hash = Hash256::decode(&tx.hash).unwrap();

            let outpoint = OutPoint {
                hash,
                index: tx.pos,
            };
            let utxo_entry = UnspentEntry {
                satoshis: tx.satoshis,
                _height: tx.height,
            };
            // add to list
            self.unspent.insert(outpoint, utxo_entry);
        }

        println!(
            "UTXO {} Loaded in {} seconds",
            self.unspent.len(),
            start.elapsed().as_millis() as f64 / 1000.0
        );
    }

    fn read_tables(&mut self) {
        // Load datastructures from the database tables
        self.load_mempool();
        self.load_tx();
        self.load_utxo();
        for c in self.collection.iter_mut() {
            c.load_txs(&mut self.conn);
        }
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
                    _height: height,
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

    fn process_tx_inputs(&mut self, tx: &Tx, tx_index: usize) {
        // Process inputs - remove from unspent
        let mut utxo_deletes: Vec<&OutPoint> = Vec::new();

        if tx_index == 0 {
            // if is coinbase - nothing to process as these won't be in the unspent
        } else {
            for vin in tx.inputs.iter() {
                // Remove from unspent
                match self.unspent.remove(&vin.prev_output) {
                    // Remove from utxo table
                    Some(_) => utxo_deletes.push(&vin.prev_output),
                    None => {}
                }
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
    }

    fn process_collection(&mut self, tx: &Tx, _height: i32, _tx_index: i32) {
        for c in self.collection.iter_mut() {
            // Check to see if we have already processed it if so quit
            if c.already_have_tx(tx.hash()) {
                continue;
            }

            // Check inputs
            // TODO: any_script_sig_matches_pattern

            if c.track_descendants() && c.is_decendant(tx) {
                // Save tx hash and write to database
                c.push(tx.hash());
                c.write_to_database(tx, &mut self.conn);
                continue;
            }

            // Check outputs
            if c.match_any_locking_script(tx) {
                // Save tx hash and write to database
                c.push(tx.hash());
                c.write_to_database(tx, &mut self.conn);
            }
        }
    }

    pub fn process_block_tx(&mut self, tx: &Tx, height: i32, tx_index: usize) {
        // Process tx as received in a block from a peer
        let hash = tx.hash();

        // Store tx - note that we only do this for tx in a block
        let tx_entry = TxEntry {
            _tx: Some(tx.clone()),
            _height: height.try_into().unwrap(),
            _size: tx.size() as u32,
        };

        if let Some(_prev) = self.txs.insert(hash, tx_entry) {
            // We must have already processed this tx in a block
            panic!("Should not get here, as it indicates that we have processed the same tx twice in a block.");
        }

        // Remove from mempool as now in block
        self.mempool.remove(&hash);

        // process inputs
        self.process_tx_inputs(tx, tx_index);

        // Process outputs
        // Note this will overwrite the unspent outpoints with height = NOT_IN_BLOCK(-1)
        // and utxo entries
        self.process_tx_outputs(tx, height);

        // Collection processing
        self.process_collection(tx, height, tx_index.try_into().unwrap());
    }

    pub fn process_block(&mut self, block: &Block, height: i32) {
        // Given a block process all the txs in it
        // Batch processing here
        let hashes: Vec<String> = block.txns.iter().map(|b| b.hash().encode()).collect();

        // Batch Delete from mempool
        self.conn
            .exec_batch(
                "DELETE FROM mempool WHERE hash = :hash;",
                hashes.iter().map(|x| params! {"hash" => x}),
            )
            .unwrap();

        let hashes_and_size: Vec<(String, u32)> = block
            .txns
            .iter()
            .map(|b| (b.hash().encode(), b.size() as u32))
            .collect();

        // Batch write tx to tx database table
        self.conn
            .exec_batch(
                "INSERT INTO tx (hash, height, txsize) VALUES (:hash, :height, :txsize)",
                hashes_and_size.iter().map(
                    |(hash, size)| params! {"hash" => hash, "height" => height, "txsize"=> size},
                ),
            )
            .unwrap();

        // now process them...
        for (tx_index, tx) in block.txns.iter().enumerate() {
            self.process_block_tx(tx, height, tx_index);
        }
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
            _tx: Some(tx.clone()),
            _age: age,
            _locktime: locktime,
            _fee: fee.try_into().unwrap(),
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

        // Process inputs
        const NOT_A_COINBASE_TX: usize = 1;

        self.process_tx_inputs(tx, NOT_A_COINBASE_TX);

        // Process outputs
        self.process_tx_outputs(tx, NOT_IN_BLOCK);

        // Collection processing
        self.process_collection(tx, NOT_IN_BLOCK, NOT_IN_BLOCK);
    }
}
