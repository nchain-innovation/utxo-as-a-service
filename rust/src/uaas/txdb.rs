use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use sv::messages::{Block, Payload, Tx};
use sv::util::{Hash256, Serializable};

use super::hexslice::HexSlice;

use mysql::prelude::*;
use mysql::PooledConn;
use mysql::*;


// Used for loading tx from mempool table
pub struct MempoolEntryDB {
    _hash: String,
}

// Used to store txs to write (in blocks)
pub struct TxEntry {
    hash: Hash256,
    height: usize,
    blockindex: u32,
    size: u32,
}

// Used for loading tx from tx table
struct TxEntryDB {
    hash: String,
}

// TxDB - wraps interface to tx and mempool database tables

pub struct TxDB {
    // Database connection
    conn: PooledConn,
    // All transactions
    pub txs: HashMap<Hash256, Hash256>,

    // mempool - transactions that are not in blocks
    pub mempool: HashMap<Hash256, Hash256>,

    // txs to remove from mempool table
    hashes_to_delete: Vec<Hash256>,

    // txs to add to tx table
    tx_entries: Vec<TxEntry>,
}

impl TxDB {
    pub fn new(conn: PooledConn) -> Self {
        TxDB {
            conn,
            txs: HashMap::new(),
            mempool: HashMap::new(),
            hashes_to_delete: Vec::new(),
            tx_entries: Vec::new(),
        }
    }

    pub fn create_tx_table(&mut self) {
        // Create tx table
        println!("Table tx not found - creating");
        self.conn
            .query_drop(
                r"CREATE TABLE tx (
                hash varchar(64) not null,
                height int unsigned not null,
                blockindex int unsigned not null,
                txsize int unsigned not null);"
                //CONSTRAINT PK_Entry PRIMARY KEY (hash));",
            )
            .unwrap();
        /*
            self.conn
            .query_drop(r"CREATE INDEX idx_tx ON tx (hash);")
            .unwrap();
        */
    }

    pub fn create_mempool_table(&mut self) {
        println!("Table mempool not found - creating");
        self.conn
            .query_drop(
                r"CREATE TABLE mempool (
                hash varchar(64) not null,
                locktime int unsigned not null,
                fee bigint unsigned not null,
                time int unsigned not null,
                tx longtext not null)",
            )
            .unwrap();
        // Note that tx longtext should be good for 4GB
        /*
        self.conn
            .query_drop(r"CREATE INDEX idx_txkey ON mempool (hash);")
            .unwrap();
        */
    }

    pub fn load_tx(&mut self) {
        // Load tx - (tx hash and height) from database
        let start = Instant::now();

        let txs: Vec<TxEntryDB> = self
            .conn
            .query_map(
                "SELECT hash FROM tx ORDER BY height",
                |hash| TxEntryDB {
                    hash,
                },
            )
            .unwrap();

        for tx in txs {
            let hash = Hash256::decode(&tx.hash).unwrap();
            self.txs.insert(hash, hash);
        }
        println!(
            "Txs {} loaded in {} seconds",
            self.txs.len(),
            start.elapsed().as_millis() as f64 / 1000.0
        );
    }

    pub fn load_mempool(&mut self) {
        // load mempool - tx hash and height from database
        let start = Instant::now();

        let txs: Vec<MempoolEntryDB> = self
            .conn
            .query_map(
                "SELECT hash FROM mempool ORDER BY time",
                |hash| MempoolEntryDB {
                    _hash: hash,
                },
            )
            .unwrap();

        for tx in txs {
            let hash = Hash256::decode(&tx._hash).unwrap();
            self.mempool.insert(hash, hash);
        }

        println!(
            "Mempool {} Loaded in {} seconds",
            self.mempool.len(),
            start.elapsed().as_millis() as f64 / 1000.0
        );
    }

    pub fn process_block(&mut self, block: &Block, height: i32) {
        // for each tx in block
        for (blockindex, tx) in block.txns.iter().enumerate() {
            let hash = tx.hash();

            // if in mempool - remove and append to list of hashes to delete
            if self.mempool.remove(&hash).is_some() {
                self.hashes_to_delete.push(hash);
            }

            // Store tx - note that we only do this for tx in a block
            let tx_entry = TxEntry {
                hash,
                height: height.try_into().unwrap(),
                blockindex: blockindex.try_into().unwrap(),
                size: tx.size() as u32,
            };

            // Write to database later
            self.tx_entries.push(tx_entry);

            if self.txs.insert(hash, hash).is_some() {
                // We must have already processed this tx in a block
                panic!("Should not get here, as it indicates that we have processed the same tx twice in a block.");
            }

        }
    }

    pub fn batch_delete_from_mempool(&mut self) {
        // Batch Delete from mempool
        self.conn
            .exec_batch(
                "DELETE FROM mempool WHERE hash = :hash;",
                self.hashes_to_delete.iter().map(|x| params! {"hash" => x.encode()}),
            )
            .unwrap();

        self.hashes_to_delete.clear();
    }

    pub fn batch_write_tx_to_table(&mut self) {
        self.conn
        .exec_batch(
            "INSERT INTO tx (hash, height, blockindex, txsize) VALUES (:hash, :height, :blockindex, :txsize)",
            self.tx_entries.iter().map(
                |tx| params! {"hash" => tx.hash.encode(), "height" => tx.height, "blockindex"=> tx.blockindex, "txsize"=> tx.size},
            ),
        )
        .unwrap();
    }

    pub fn add_to_mempool(&mut self, tx: &Tx, fee: i64) {
        let hash = tx.hash();
        let age = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let locktime = tx.lock_time;
        // Add it to the mempool
        self.mempool.insert(hash, hash);

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
    }

    pub fn tx_exists(&self, hash: Hash256) -> bool {
        // Return true if txid is in txs or mempool
        self.txs.contains_key(&hash) || self.mempool.contains_key(&hash)
    }
}
