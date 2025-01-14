use std::collections::HashMap;
use std::sync::mpsc;

use std::time::Instant;

use chain_gang::messages::OutPoint;
use chain_gang::util::Hash256;

use mysql::prelude::*;
use mysql::PooledConn;
// use mysql::*;

use super::database::{DBOperationType, UtxoEntryDB};

// Used to store the unspent txs (UTXO)
#[derive(Clone)]
pub struct UtxoEntry {
    satoshis: i64,
    // lock_script: Script, - have seen some very large script lengths here - removed for now
    height: i32, // use NOT_IN_BLOCK -1 to indicate that tx is not in block
    #[allow(dead_code)] // pubkeyhash
    pubkeyhash: String,
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

    // Channel to database
    tx: mpsc::Sender<DBOperationType>,
}

impl Utxo {
    pub fn new(conn: PooledConn, tx: mpsc::Sender<DBOperationType>) -> Self {
        Utxo {
            utxo: HashMap::new(),
            conn,
            utxo_entries: HashMap::new(),
            utxo_deletes: Vec::new(),
            tx,
        }
    }

    pub fn create_table(&mut self) {
        // Create Utxo table
        // utxo
        log::info!("Table utxo not found - creating");
        self.conn
            .query_drop(
                r"CREATE TABLE utxo (
                hash varchar(64) not null,
                pos int unsigned not null,
                satoshis bigint unsigned not null,
                height int not null,
                pubkeyhash varchar(64),
                CONSTRAINT PK_Entry PRIMARY KEY (hash, pos));",
            )
            .unwrap();

        self.conn
            .query_drop(r"CREATE INDEX speed_key ON utxo (pubkeyhash);")
            .unwrap();
    }

    pub fn load_utxo(&mut self) {
        // load outpoints from database
        let start = Instant::now();

        let txs: Vec<UtxoEntryDB> = self
            .conn
            .query_map(
                "SELECT * FROM utxo",
                |(hash, pos, satoshis, height, pubkeyhash)| UtxoEntryDB {
                    hash,
                    pos,
                    satoshis,
                    height,
                    pubkeyhash,
                },
            )
            .unwrap();

        // Load entries into utxo struct
        for entry in txs {
            let hash = Hash256::decode(&entry.hash).unwrap();

            let outpoint = OutPoint {
                hash,
                index: entry.pos,
            };
            let utxo_entry = UtxoEntry {
                satoshis: entry.satoshis,
                height: entry.height,
                pubkeyhash: entry.pubkeyhash,
            };
            // add to list
            self.utxo.insert(outpoint, utxo_entry);
        }

        // How long did it take
        log::info!(
            "UTXO {} Loaded in {} seconds",
            self.utxo.len(),
            start.elapsed().as_millis() as f64 / 1000.0
        );
    }

    pub fn add(
        &mut self,
        hash: Hash256,
        index: usize,
        satoshis: i64,
        height: i32,
        pubkeyhash: &str,
    ) {
        // add a utxo outpoint, prepare a record to be written to database
        let outpoint = OutPoint {
            hash,
            index: index.try_into().unwrap(),
        };

        let new_entry = UtxoEntry {
            satoshis,
            // lock_script: vout.lock_script.clone(),
            height,
            pubkeyhash: pubkeyhash.to_string(),
        };
        // add to utxo list
        self.utxo.insert(outpoint.clone(), new_entry);

        // Record for batch write to utxo table
        let utxo_entry = UtxoEntryDB {
            hash: hash.encode(),
            pos: index.try_into().unwrap(),
            satoshis,
            height,
            pubkeyhash: pubkeyhash.to_string(),
        };
        self.utxo_entries.insert(outpoint, utxo_entry);
    }

    pub fn delete(&mut self, outpoint: &OutPoint) {
        // Remove from utxo
        if self.utxo.remove(outpoint).is_some() {
            // Remove from utxo table
            self.utxo_deletes.push(outpoint.clone());
            // also remove from utxo entries if present
            self.utxo_entries.remove(outpoint);
        }
    }

    pub fn get_satoshis(&self, outpoint: &OutPoint) -> Option<i64> {
        // Return the satoshis associated with this outpoint
        self.utxo.get(outpoint).map(|v| v.satoshis)
    }

    pub fn update_db(&mut self) {
        // bulk/batch write tx output to utxo table
        let request: Vec<UtxoEntryDB> = self.utxo_entries.clone().into_values().collect();
        self.tx
            .send(DBOperationType::UtxoBatchWrite(request))
            .unwrap();
        self.utxo_entries.clear();

        // bulk/batch delete utxo table entries
        self.tx
            .send(DBOperationType::UtxoBatchDelete(self.utxo_deletes.clone()))
            .unwrap();
        self.utxo_deletes.clear();
    }

    pub fn handle_orphan_block(&mut self, height: u32) {
        // Remove utxo of this block height
        self.tx.send(DBOperationType::UtxoDelete(height)).unwrap();

        let height_as_i32: i32 = height.try_into().unwrap();
        // Remove transactions at this height
        self.utxo
            .retain(|_outpoint, entry| entry.height != height_as_i32);
    }
}
