use std::time::Instant;

use mysql::{prelude::*, PooledConn, *};

use crate::{
    config::{CollectionConfig, Config},
    uaas::hexslice::HexSlice,
};
use anyhow::{anyhow, Result};
use chain_gang::{
    address::{addr_decode, AddressType},
    messages::{Payload, Tx},
    network::Network,
    transaction::p2pkh,
    util::{Hash256, Serializable},
};
use regex::Regex;
use retry::{delay, retry};

/// Given an address return a locking script in hexstr format
fn address_to_lock_script(address: &str) -> Result<String> {
    let (hash160, address_type) = addr_decode(address, Network::BSV_Testnet)?;
    assert!(address_type == AddressType::P2PKH);
    let script = p2pkh::create_lock_script(&hash160);
    Ok(hex::encode(script.0))
}

/// Database interface used by all collections
///
///
#[derive(Debug)]
pub struct CollectionDatabase {
    // Retry database connections
    ms_delay: u64,
    retries: usize,
    conn: PooledConn,
}

impl CollectionDatabase {
    pub fn new(conn: PooledConn, config: &Config) -> Self {
        CollectionDatabase {
            ms_delay: config.database.ms_delay,
            retries: config.database.retries,
            conn,
        }
    }

    pub fn create_table(&self, conn: &mut PooledConn) {
        log::info!("Table collection not found - creating");

        let table = "CREATE TABLE collection (hash varchar(64), name varchar(64), tx longtext, CONSTRAINT PK_Entry PRIMARY KEY (hash, name));";
        conn.query_drop(table).unwrap();

        // create index
        let index = "CREATE INDEX collect_key ON collection (hash, name);";
        conn.query_drop(index).unwrap();
    }

    pub fn load_txs(&mut self, collection_name: &str) -> Vec<Hash256> {
        // load txs- tx hash from database
        let start = Instant::now();
        let table = format!(
            "SELECT hash FROM collection WHERE name = '{}';",
            collection_name
        );
        let txs: Vec<String> = self.conn.query_map(table, |hash| hash).unwrap();

        let retval: Vec<Hash256> = txs.iter().map(|x| Hash256::decode(x).unwrap()).collect();

        log::info!(
            "Collection {} Loaded {} in {} seconds",
            collection_name,
            retval.len(),
            start.elapsed().as_millis() as f64 / 1000.0
        );
        retval
    }

    pub fn write_tx_to_database(&mut self, collection_name: &str, tx: &Tx) {
        let hash = tx.hash().encode();
        // Write the tx as hexstr
        let mut b = Vec::with_capacity(tx.size());
        tx.write(&mut b).unwrap();
        let tx_hex = format!("{}", HexSlice::new(&b));

        let collection_insert = format!(
            "INSERT INTO collection VALUES ('{}', '{}', '{}');",
            &hash, collection_name, tx_hex,
        );

        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || self.conn.exec_drop(&collection_insert, Params::Empty),
        );
        result.unwrap();
    }
}

pub struct WorkingCollection {
    // this is a collection that also maintains a list of tx hashes that it has used
    pub collection: CollectionConfig,
    pub txs: Vec<Hash256>,
    // No point to the Collection if there is no locking_script_regex
    // Actually there is for is_uaas_broadcast txs
    locking_script_regex: Option<Regex>,
}

impl WorkingCollection {
    pub fn new(collection: CollectionConfig) -> Result<Self> {
        if let Some(ref addr) = collection.address {
            // address -> regex locking script
            let pattern = address_to_lock_script(addr)?;
            let locking_script_regex = Regex::new(&pattern)?;
            return Ok(WorkingCollection {
                collection: collection.clone(),
                txs: Vec::new(),
                locking_script_regex: Some(locking_script_regex),
            });
        }

        if let Some(ref pattern) = collection.locking_script_pattern {
            let locking_script_regex = Regex::new(pattern)?;

            return Ok(WorkingCollection {
                collection: collection.clone(),
                txs: Vec::new(),
                locking_script_regex: Some(locking_script_regex),
            });
        }
        Err(anyhow!(
            "Incorrect Collection configuration {:?}",
            &collection
        ))
    }

    // Create a special form of collection just to catch broadcasts
    pub fn create_broadcast_collection() -> Self {
        let broadcast_collection = CollectionConfig {
            name: "broadcast".to_string(),
            track_descendants: false,
            address: None,
            locking_script_pattern: None,
        };

        WorkingCollection {
            // this is a collection that also maintains a list of tx hashes that it has used
            collection: broadcast_collection,
            txs: Vec::new(),
            // No point to the Collection if there is no locking_script_regex
            // Actually there is for is_uaas_broadcast txs
            locking_script_regex: None,
        }
    }

    pub fn name(&self) -> &str {
        self.collection.name.as_str()
    }

    pub fn track_descendants(&self) -> bool {
        self.collection.track_descendants
    }

    pub fn have_tx(&self, hash: Hash256) -> bool {
        // Return true if we already have this tx hash
        self.txs.iter().any(|x| x == &hash)
    }

    pub fn match_any_locking_script(&self, tx: &Tx) -> bool {
        if let Some(locking_script_regex) = &self.locking_script_regex {
            for vout in &tx.outputs {
                // Convert the script into hexstring
                let script_hex = format!("{}", HexSlice::new(&vout.lock_script.0));
                // Pattern match here
                if locking_script_regex.is_match(&script_hex) {
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
