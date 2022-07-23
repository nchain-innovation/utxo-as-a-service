use std::collections::HashMap;

use std::time::Instant;

use sv::messages::OutPoint;
use sv::util::Hash256;

use mysql::prelude::*;
use mysql::PooledConn;
use mysql::*;

// Used to store the unspent txs (UTXO)
pub struct UtxoEntry {
    satoshis: i64,
    // lock_script: Script, - have seen some very large script lengths here - removed for now
    _height: i32, // use NOT_IN_BLOCK -1 to indicate that tx is not in block
}

// UtxoEntry - used to store data into utxo table
#[derive(Debug)]
struct UtxoEntryDB {
    pub hash: String,
    pub pos: u32,
    pub satoshis: i64,
    pub height: i32,
}

// provides access to utxo state and wraps interface to utxo table
pub struct Utxo {
    // Unspent tx
    utxo: HashMap<OutPoint, UtxoEntry>,
    // Database connection
    conn: PooledConn,

    // Record for batch write to utxo table
    utxo_entries: HashMap<OutPoint, UtxoEntryDB>,

    // Process inputs - remove from utxo
    utxo_deletes: Vec<OutPoint>,
}

impl Utxo {
    pub fn new(conn: PooledConn) -> Self {
        Utxo {
            utxo: HashMap::new(),
            conn,
            utxo_entries: HashMap::new(),
            utxo_deletes: Vec::new(),
        }
    }

    pub fn create_table(&mut self) {
        // Create Utxo table
        // utxo
        println!("Table utxo not found - creating");
        self.conn
            .query_drop(
                r"CREATE TABLE utxo (
                hash varchar(64) not null,
                pos int unsigned not null,
                satoshis bigint unsigned not null,
                height int not null);"
                // CONSTRAINT PK_Entry PRIMARY KEY (hash, pos));",
            )
            .unwrap();
        /*
        self.conn
            .query_drop(r"CREATE INDEX idx_key ON utxo (hash, pos);")
            .unwrap();
        */
    }

    pub fn load_utxo(&mut self) {
        // load outpoints from database
        let start = Instant::now();

        let txs: Vec<UtxoEntryDB> = self
            .conn
            .query_map("SELECT * FROM utxo", |(hash, pos, satoshis, height)| {
                UtxoEntryDB {
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
            let utxo_entry = UtxoEntry {
                satoshis: tx.satoshis,
                _height: tx.height,
            };
            // add to list
            self.utxo.insert(outpoint, utxo_entry);
        }

        println!(
            "UTXO {} Loaded in {} seconds",
            self.utxo.len(),
            start.elapsed().as_millis() as f64 / 1000.0
        );
    }

    pub fn add(&mut self, hash: Hash256, index: usize, satoshis: i64, height: i32) {
        // add a utxo outpoint, prepare a record to be written to database
        let outpoint = OutPoint {
            hash,
            index: index.try_into().unwrap(),
        };

        let new_entry = UtxoEntry {
            satoshis,
            // lock_script: vout.lock_script.clone(),
            _height: height,
        };
        // add to utxo list
        self.utxo.insert(outpoint.clone(), new_entry);

        // Record for batch write to utxo table
        let utxo_entry = UtxoEntryDB {
            hash: hash.encode(),
            pos: index.try_into().unwrap(),
            satoshis,
            height,
        };
        self.utxo_entries.insert(outpoint, utxo_entry);
    }

    pub fn delete(&mut self, outpoint: &OutPoint) {
        // Remove from utxo
        match self.utxo.remove(outpoint) {
            // Remove from utxo table
            Some(_) => {
                self.utxo_deletes.push(outpoint.clone());
                // also remove from utxo entries if present
                self.utxo_entries.remove(outpoint);
            }
            None => {}
        }
    }

    pub fn get_satoshis(&self, outpoint: &OutPoint) -> Option<i64> {
        // Return the satoshis associated with this outpoint
        self.utxo.get(outpoint).map(|v| v.satoshis)
    }

    pub fn update_db(&mut self) {
        // bulk/batch write tx output to utxo table
        self.conn
        .exec_batch(
            //"INSERT OVERWRITE utxo (hash, pos, satoshis, height) VALUES (:hash, :pos, :satoshis, :height);",
            "REPLACE INTO utxo (hash, pos, satoshis, height) VALUES (:hash, :pos, :satoshis, :height);",
            self.utxo_entries
                .iter()
                .map(|(_key, x)| params! {
                    "hash" => x.hash.as_str(), "pos" => x.pos, "satoshis" => x.satoshis, "height" => x.height
                }),
        )
        .unwrap();

        self.utxo_entries.clear();

        // bulk/batch delete utxo table entries
        self.conn
            .exec_batch(
                "DELETE FROM utxo WHERE hash = :hash AND pos = :pos;",
                self.utxo_deletes
                    .iter()
                    .map(|x| params! {"hash" => x.hash.encode(), "pos" => x.index}),
            )
            .unwrap();

        self.utxo_deletes.clear();
    }
}
