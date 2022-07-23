use std::cmp;
use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use mysql::prelude::*;
use mysql::Pool;
use mysql::PooledConn;
use mysql::*;

use super::hexslice::HexSlice;

use sv::messages::{Block, Payload, Tx, TxOut};
use sv::util::{Hash256, Serializable};

use super::utxo::Utxo;
use crate::config::Config;
use crate::uaas::collection::WorkingCollection;

/*
    in - unlock_script - script sig
    out - lock_script - script public key
*/

const NOT_IN_BLOCK: i32 = -1; // use -1 to indicate that this tx is not in block

// Used to store all txs (in mempool)
pub struct MempoolEntry {
    _tx: Option<Tx>,
    _locktime: u32,
    _fee: u64,
    _age: u64,
}

// Used for loading tx from mempool table
pub struct MempoolEntryDB {
    pub hash: String,
    locktime: u32,
    fee: u64,
    age: u64,
    _tx: Vec<u8>,
}

// Used to store all txs (in blocks)
pub struct TxEntry {
    _tx: Option<Tx>,
    _height: usize,
    _blockindex: u32,
    _size: u32,
}

// Used for loading tx from tx table
struct TxEntryDB {
    pub hash: String,
    pub height: usize,
    pub blockindex: u32,
    pub size: u32,
}

pub struct TxAnalyser {
    // All transactions
    txs: HashMap<Hash256, TxEntry>,

    // mempool - transactions that are not in blocks
    mempool: HashMap<Hash256, MempoolEntry>,

    // Unspent tx - make public so logic can write to database when in ready state
    pub utxo: Utxo,

    // Database connection
    conn: PooledConn,

    // Collections
    collection: Vec<WorkingCollection>,
}

impl TxAnalyser {
    pub fn new(config: &Config, pool: Pool) -> Self {
        let tx_conn = pool.get_conn().unwrap();
        let utxo_conn = pool.get_conn().unwrap();
        let mut txanal = TxAnalyser {
            txs: HashMap::new(),
            mempool: HashMap::new(),
            utxo: Utxo::new(utxo_conn),
            conn: tx_conn,
            collection: Vec::new(),
        };

        // Load the collections
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
                    blockindex int unsigned,
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
                    time int unsigned,
                    tx longtext)",
                )
                .unwrap();
            // Note that tx longtext should be good for 4GB
            self.conn
                .query_drop(r"CREATE INDEX idx_txkey ON mempool (hash);")
                .unwrap();
        }

        // utxo
        if !tables.iter().any(|x| x.as_str() == "utxo") {
            self.utxo.create_table();
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
        // Load tx - (tx hash and height) from database
        let start = Instant::now();

        let txs: Vec<TxEntryDB> = self
            .conn
            .query_map(
                "SELECT * FROM tx ORDER BY height",
                |(hash, height, blockindex, size)| TxEntryDB {
                    hash,
                    height,
                    blockindex,
                    size,
                },
            )
            .unwrap();

        for tx in txs {
            let tx_entry = TxEntry {
                _tx: None,
                _height: tx.height,
                _blockindex: tx.blockindex,
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

        let txs: Vec<MempoolEntryDB> = self
            .conn
            .query_map(
                "SELECT * FROM mempool ORDER BY time",
                |(hash, locktime, fee, time, tx)| MempoolEntryDB {
                    hash,
                    locktime,
                    fee,
                    age: time,
                    _tx: tx,
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

    fn read_tables(&mut self) {
        // Load datastructures from the database tables
        self.load_mempool();
        self.load_tx();
        self.utxo.load_utxo();
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
        // process the tx outputs and place them in the utxo

        let hash = tx.hash();

        // Process outputs - add to utxo
        for (index, vout) in tx.outputs.iter().enumerate() {
            if self.is_spendable(vout) {
                self.utxo.add(hash, index, vout.satoshis, height);
            }
        }
    }

    fn process_tx_inputs(&mut self, tx: &Tx, blockindex: usize) {
        if blockindex == 0 {
            // if is coinbase - nothing to process as these won't be in the utxo
        } else {
            for vin in tx.inputs.iter() {
                self.utxo.delete(&vin.prev_output);
            }
        }
    }

    fn process_collection(&mut self, tx: &Tx, _height: i32, _blockindex: i32) {
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

    pub fn process_block_tx(&mut self, tx: &Tx, height: i32, blockindex: usize) {
        // Process tx as received in a block from a peer
        let hash = tx.hash();

        // Store tx - note that we only do this for tx in a block
        let tx_entry = TxEntry {
            _tx: Some(tx.clone()),
            _height: height.try_into().unwrap(),
            _blockindex: blockindex.try_into().unwrap(),
            _size: tx.size() as u32,
        };

        if let Some(_prev) = self.txs.insert(hash, tx_entry) {
            // We must have already processed this tx in a block
            panic!("Should not get here, as it indicates that we have processed the same tx twice in a block.");
        }

        // Remove from mempool as now in block
        self.mempool.remove(&hash);

        // process inputs
        self.process_tx_inputs(tx, blockindex);

        // Process outputs
        // Note this will overwrite the utxo outpoints with height = NOT_IN_BLOCK(-1)
        // and utxo entries
        self.process_tx_outputs(tx, height);

        // Collection processing
        self.process_collection(tx, height, blockindex.try_into().unwrap());
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

        let hash_blockindex_size: Vec<(String, u32, u32)> = block
            .txns
            .iter()
            .enumerate()
            .map(|(i, b)| (b.hash().encode(), i as u32, b.size() as u32))
            .collect();

        // Batch write tx to tx database table
        self.conn
            .exec_batch(
                "INSERT INTO tx (hash, height, blockindex, txsize) VALUES (:hash, :height, :blockindex, :txsize)",
                hash_blockindex_size.iter().map(
                    |(hash, blockindex, size)| params! {"hash" => hash, "height" => height, "blockindex"=> blockindex, "txsize"=> size},
                ),
            )
            .unwrap();

        // now process them...
        for (blockindex, tx) in block.txns.iter().enumerate() {
            self.process_block_tx(tx, height, blockindex);
        }
    }

    fn calc_fee(&self, tx: &Tx) -> i64 {
        // Given the tx attempt to determine the fee, return 0 if unable to calculate
        let mut inputs = 0i64;
        for vin in tx.inputs.iter() {
            if let Some(satoshis) = self.utxo.get_satoshis(&vin.prev_output) {
                inputs += satoshis;
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

        // Write the tx as hexstr
        let mut b = Vec::with_capacity(tx.size());
        tx.write(&mut b).unwrap();
        let tx_hex = format!("{}", HexSlice::new(&b));

        // Write mempool entry to database
        let mempool_insert = format!(
            "INSERT INTO mempool VALUES ('{}', {}, {}, {},'{}');",
            &hash.encode(),
            &locktime,
            &fee,
            &age,
            &tx_hex,
        );
        self.conn.exec_drop(&mempool_insert, Params::Empty).expect(
            "Problem writing to mempool table. Check that tx field is present in mempool table.\n",
        );

        // Process inputs
        const NOT_A_COINBASE_TX: usize = 1;

        self.process_tx_inputs(tx, NOT_A_COINBASE_TX);

        // Process outputs
        self.process_tx_outputs(tx, NOT_IN_BLOCK);

        // Collection processing
        self.process_collection(tx, NOT_IN_BLOCK, NOT_IN_BLOCK);
    }

    pub fn tx_exists(&self, hash: Hash256) -> bool {
        // Return true if txid is in txs or mempool
        self.txs.contains_key(&hash) || self.mempool.contains_key(&hash)
    }
}
