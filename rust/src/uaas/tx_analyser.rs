use std::{cmp, sync::mpsc};

use mysql::{prelude::*, Pool, PooledConn};

use chain_gang::{
    messages::{Block, Tx, TxOut},
    script::Script,
    util::Hash256,
};

use crate::{
    config::{CollectionConfig, Config},
    dynamic_config::DynamicConfig,
    uaas::{
        collection::{CollectionDatabase, WorkingCollection},
        database::DBOperationType,
        txdb::TxDB,
        utxo::Utxo,
    },
};
/*
    in - unlock_script - script sig
    out - lock_script - script public key
*/

const NOT_IN_BLOCK: i32 = -1; // use -1 to indicate that this tx is not in block

// Given a locking script return the hash of the public key, as hex str
// Assuming "p2pkh", locking_script_pattern = "76a914[0-9a-f]{40}88ac"
fn script_to_pubkeyhash(locking_script: &Script) -> String {
    if locking_script.0.len() == 25 {
        let hexstr = hex::encode(&locking_script.0);
        if hexstr[0..6] == *"76a914" && hexstr[46..] == *"88ac" {
            return hexstr[6..46].to_string();
        }
    }
    "unknown".to_string()
}

pub struct TxAnalyser {
    save_txs: bool,
    // Database interface
    pub txdb: TxDB,
    // Unspent tx - make public so logic can write to database when in ready state
    pub utxo: Utxo,
    // Database connection
    conn: PooledConn,
    // Collections
    collection: Vec<WorkingCollection>,
    collection_db: CollectionDatabase,
    dynamic_config: DynamicConfig,
}

impl TxAnalyser {
    pub fn new(config: &Config, pool: Pool, tx: mpsc::Sender<DBOperationType>) -> Self {
        // database connections
        let tx_conn = pool.get_conn().unwrap();
        let utxo_conn = pool.get_conn().unwrap();
        let txdb_conn = pool.get_conn().unwrap();
        let collection_conn = pool.get_conn().unwrap();

        let save_txs = config.get_network_settings().save_txs;
        let dynamic_config = DynamicConfig::new(config);
        let mut collection: Vec<WorkingCollection> = Vec::new();

        // Load the collections
        for c in &config.collection {
            match WorkingCollection::new(c.clone()) {
                Ok(wc) => collection.push(wc),
                Err(e) => println!("Error parsing collection {:?}", e),
            }
        }
        // load the dynamic collection
        for c in &dynamic_config.collection {
            match WorkingCollection::new(c.clone()) {
                Ok(wc) => collection.push(wc),
                Err(e) => println!("Error parsing collection {:?}", e),
            }
        }
        TxAnalyser {
            save_txs,
            txdb: TxDB::new(txdb_conn, tx.clone(), save_txs),
            utxo: Utxo::new(utxo_conn, tx),
            conn: tx_conn,
            collection,
            collection_db: CollectionDatabase::new(collection_conn, config),
            dynamic_config: dynamic_config.clone(),
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

        if self.save_txs && !tables.iter().any(|x| x.as_str() == "tx") {
            self.txdb.create_tx_table();
        }

        if !tables.iter().any(|x| x.as_str() == "mempool") {
            self.txdb.create_mempool_table();
        }

        // utxo
        if !tables.iter().any(|x| x.as_str() == "utxo") {
            self.utxo.create_table();
        }

        // Collection table
        if !tables.iter().any(|x| x.as_str() == "collection") {
            self.collection_db.create_table(&mut self.conn);
        }
    }

    fn read_tables(&mut self) {
        // Load datastructures from the database tables
        self.txdb.load_mempool();
        if self.save_txs {
            self.txdb.load_tx();
        }

        self.utxo.load_utxo();
        for c in self.collection.iter_mut() {
            c.txs = self.collection_db.load_txs(c.name());
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
                // Get public key hash from locking script
                let pubkeyhash = script_to_pubkeyhash(&vout.lock_script);
                self.utxo
                    .add(hash, index, vout.satoshis, height, &pubkeyhash);
            }
        }
    }

    fn process_tx_inputs(&mut self, tx: &Tx, blockindex: usize) {
        if blockindex == 0 {
            // if is coinbase (blockindex 0)- nothing to process as these won't be in the utxo
        } else {
            let _ = tx
                .inputs
                .iter()
                .map(|vin| self.utxo.delete(&vin.prev_output));
        }
    }

    fn process_collection(&mut self, tx: &Tx) {
        for c in self.collection.iter_mut() {
            // Check to see if we have already processed it if so quit
            if c.have_tx(tx.hash()) {
                return;
            }

            if (c.track_descendants() && c.is_decendant(tx)) || c.match_any_locking_script(tx) {
                // Save tx hash and write to database
                c.push(tx.hash());
                self.collection_db.write_tx_to_database(c.name(), tx);
                return;
            }
        }
    }

    pub fn process_block_tx(&mut self, tx: &Tx, height: i32, blockindex: usize) {
        // Process tx as received in a block from a peer

        // process inputs
        self.process_tx_inputs(tx, blockindex);

        // Process outputs
        // Note this will overwrite the utxo outpoints with height = NOT_IN_BLOCK(-1)
        // and utxo entries
        self.process_tx_outputs(tx, height);

        // Collection processing
        self.process_collection(tx);
    }

    pub fn process_block(&mut self, block: &Block, height: i32) {
        // Given a block process all the txs in it

        self.txdb.process_block(block, height);

        // now process Txs...
        let _ = block
            .txns
            .iter()
            .enumerate()
            .map(|(blockindex, tx)| self.process_block_tx(tx, height, blockindex));

        // Do db writes here
        self.flush_database_cache();
    }

    pub fn flush_database_cache(&mut self) {
        self.utxo.update_db();
        self.txdb.batch_delete_from_mempool();
        if self.save_txs {
            self.txdb.batch_write_tx_to_table();
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
        //log::info!("fee={} ({} - {})", fee, inputs, outputs);
        // Don't return a negative fee, it must be at least 0
        cmp::max(0i64, fee)
    }

    pub fn process_standalone_tx(&mut self, tx: &Tx) {
        // Process standalone tx as we receive them.
        // Note standalone tx are txs that are not in a block.
        let fee = self.calc_fee(tx);

        self.txdb.add_to_mempool(tx, fee);

        // Process inputs
        const NOT_A_COINBASE_TX: usize = 1;

        self.process_tx_inputs(tx, NOT_A_COINBASE_TX);

        // Process outputs
        self.process_tx_outputs(tx, NOT_IN_BLOCK);

        // Collection processing
        self.process_collection(tx);
    }

    pub fn tx_exists(&self, hash: Hash256) -> bool {
        // Return true if txid is in txs or mempool
        // As we may not store all txs we assume that a collection has been setup for any that we are
        // interested in and so we have to search the collections
        self.txdb.tx_exists(hash) || self.collection.iter().any(|c| c.have_tx(hash))
    }

    pub fn handle_orphan_block(&mut self, height: u32) {
        self.txdb.handle_orphan_block(height);
        self.utxo.handle_orphan_block(height);
    }

    fn is_name_in_collection(&self, name: &str) -> bool {
        self.collection.iter().any(|c| c.collection.name == name)
    }

    fn is_name_in_dynamic_collection(&self, name: &str) -> bool {
        self.dynamic_config
            .collection
            .iter()
            .any(|c| c.name == name)
    }

    pub fn add_monitor(&mut self, monitor: CollectionConfig) {
        log::info!("add_monitor {:?}", &monitor);
        // Check name is not in collection
        if !self.is_name_in_collection(&monitor.name) {
            // add to collection
            match WorkingCollection::new(monitor.clone()) {
                Ok(wc) => {
                    self.collection.push(wc);
                    // add to dynamic config
                    self.dynamic_config.add(&monitor);
                }
                Err(e) => println!("Error parsing collection {:?}", e),
            }
        }
    }

    pub fn delete_monitor(&mut self, monitor_name: &str) {
        log::info!("delete_monitor {}", monitor_name);
        // Check is in collection & dynamic config
        if self.is_name_in_dynamic_collection(monitor_name) {
            // Delete from to collection
            match self
                .collection
                .iter()
                .position(|c| c.collection.name == monitor_name)
            {
                Some(index) => {
                    self.collection.remove(index);
                }
                None => println!("Error indexing collection {}", monitor_name),
            }
            // Delete from dynamic config
            self.dynamic_config.delete(monitor_name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_script_to_pubkeyhash() {
        //fn script_to_pubkeyhash(locking_script: &Script) -> String {
        //"asm": "OP_DUP OP_HASH160 7c78584493557fac782023a4ad591b64545929d9 OP_EQUALVERIFY OP_CHECKSIG",

        let encoded_script =
            hex::decode("76a9147c78584493557fac782023a4ad591b64545929d988ac").unwrap();
        let locking_script = Script(encoded_script);
        let result = script_to_pubkeyhash(&locking_script);
        println!("{}", &result);

        assert_eq!(&result, "7c78584493557fac782023a4ad591b64545929d9");
    }
}
