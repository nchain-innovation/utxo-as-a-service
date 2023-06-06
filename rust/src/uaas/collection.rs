use std::time::Instant;

use mysql::{prelude::*, PooledConn, *};
use serde::Deserialize;

use crate::{config::Config, uaas::hexslice::HexSlice};
use chain_gang::{
    messages::{Payload, Tx},
    util::{Hash256, Serializable},
};
use regex::Regex;
use retry::{delay, retry};

#[derive(Debug, Deserialize, Clone)]
pub struct Collection {
    pub name: String,
    pub track_descendants: bool,
    pub locking_script_pattern: Option<String>,
}

pub struct WorkingCollection {
    // this is a collection that also maintains a list of tx hashes that it has used
    collection: Collection,
    txs: Vec<Hash256>,
    locking_script_regex: Option<Regex>,
    // self.ms_delay).take(self.retries),
    // Retry database connections
    ms_delay: u64,
    retries: usize,
}

impl WorkingCollection {
    pub fn new(collection: Collection, config: Config) -> Self {
        let locking_script_regex = collection
            .locking_script_pattern
            .as_ref()
            .map(|pattern| Regex::new(pattern).unwrap());

        WorkingCollection {
            collection,
            txs: Vec::new(),
            locking_script_regex,
            ms_delay: config.database.ms_delay,
            retries: config.database.retries,
        }
    }

    pub fn name(&self) -> &str {
        self.collection.name.as_str()
    }

    pub fn track_descendants(&self) -> bool {
        self.collection.track_descendants
    }

    pub fn already_have_tx(&self, hash: Hash256) -> bool {
        // Return true if we already have this tx hash
        self.txs.iter().any(|x| x == &hash)
    }

    pub fn create_table(&self, conn: &mut PooledConn) {
        let table = format!(
            "CREATE TABLE {} (hash varchar(64), tx longtext, CONSTRAINT PK_Entry PRIMARY KEY (hash));",
            self.collection.name
        );
        conn.query_drop(table).unwrap();

        // create index
        let index = format!(
            "CREATE INDEX collect_key ON {} (hash);",
            self.collection.name
        );
        conn.query_drop(index).unwrap();
    }

    pub fn load_txs(&mut self, conn: &mut PooledConn) {
        // load txs- tx hash from database
        let start = Instant::now();
        let table = format!("SELECT hash FROM {};", self.collection.name);
        let txs: Vec<String> = conn.query_map(table, |hash| hash).unwrap();

        for hash_str in txs {
            let hash = Hash256::decode(&hash_str).unwrap();
            self.txs.push(hash);
        }
        log::info!(
            "Collection {} Loaded {} in {} seconds",
            self.name(),
            self.txs.len(),
            start.elapsed().as_millis() as f64 / 1000.0
        );
    }

    pub fn write_to_database(&self, tx: &Tx, conn: &mut PooledConn) {
        let hash = tx.hash().encode();
        // Write the tx as hexstr
        let mut b = Vec::with_capacity(tx.size());
        tx.write(&mut b).unwrap();
        let tx_hex = format!("{}", HexSlice::new(&b));

        let collection_insert = format!(
            "INSERT INTO {} VALUES ('{}', '{}');",
            self.collection.name, &hash, tx_hex,
        );

        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || {
                conn.exec_drop(&collection_insert, Params::Empty)
            },
        );

        result.unwrap();
    }

    pub fn match_any_locking_script(&self, tx: &Tx) -> bool {
        if let Some(regex) = &self.locking_script_regex {
            for vout in &tx.outputs {
                // Convert the script into hexstring
                let script_hex = format!("{}", HexSlice::new(&vout.lock_script.0));
                // Pattern match here
                if regex.is_match(&script_hex) {
                    return true;
                }
            }
        }
        false
    }

    pub fn push(&mut self, hash: Hash256) {
        // Add to our list of known txs
        self.txs.push(hash);
    }

    pub fn is_decendant(&self, tx: &Tx) -> bool {
        // Return true if transaction is a decendant of a known `collection` transaction.
        for vin in &tx.inputs {
            if self.txs.iter().any(|x| x == &vin.prev_output.hash) {
                return true;
            }
        }
        false
    }
}
