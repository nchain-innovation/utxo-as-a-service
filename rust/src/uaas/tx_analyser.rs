use std::{cmp, sync::mpsc};

use mysql::{prelude::*, Pool, PooledConn};

use chain_gang::{
    messages::{Block, Tx, TxOut},
    util::Hash256,
};

use crate::{
    config::Config,
    uaas::{collection::WorkingCollection, database::DBOperationType, txdb::TxDB, utxo::Utxo},
};
/*
    in - unlock_script - script sig
    out - lock_script - script public key
*/

const NOT_IN_BLOCK: i32 = -1; // use -1 to indicate that this tx is not in block

pub struct TxAnalyser {
    // Database interface
    pub txdb: TxDB,
    // Unspent tx - make public so logic can write to database when in ready state
    pub utxo: Utxo,
    // Database connection
    conn: PooledConn,
    // Collections
    collection: Vec<WorkingCollection>,
}

impl TxAnalyser {
    pub fn new(config: &Config, pool: Pool, tx: mpsc::Sender<DBOperationType>) -> Self {
        // database connections
        let tx_conn = pool.get_conn().unwrap();
        let utxo_conn = pool.get_conn().unwrap();
        let txdb_conn = pool.get_conn().unwrap();

        let mut txanal = TxAnalyser {
            txdb: TxDB::new(txdb_conn, tx.clone()),
            utxo: Utxo::new(utxo_conn, tx),
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
            self.txdb.create_tx_table();
        }

        if !tables.iter().any(|x| x.as_str() == "mempool") {
            self.txdb.create_mempool_table();
        }

        // utxo
        if !tables.iter().any(|x| x.as_str() == "utxo") {
            self.utxo.create_table();
        }

        // Collection tables
        for c in &self.collection {
            let name = c.name();
            if !tables.iter().any(|x| x.as_str() == name) {
                log::info!("Table collection {} not found - creating", name);
                c.create_table(&mut self.conn);
            }
        }
    }

    fn read_tables(&mut self) {
        // Load datastructures from the database tables
        self.txdb.load_mempool();
        self.txdb.load_tx();
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

        self.txdb.process_block(block, height);

        // now process them...
        for (blockindex, tx) in block.txns.iter().enumerate() {
            self.process_block_tx(tx, height, blockindex);
        }

        // Do db writes here
        self.utxo.update_db();
        self.txdb.batch_delete_from_mempool();
        self.txdb.batch_write_tx_to_table();
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
        self.process_collection(tx, NOT_IN_BLOCK, NOT_IN_BLOCK);
    }

    pub fn tx_exists(&self, hash: Hash256) -> bool {
        // Return true if txid is in txs or mempool
        self.txdb.tx_exists(hash)
    }

    pub fn handle_orphan_block(&mut self, height: u32) {
        self.txdb.handle_orphan_block(height);
        self.utxo.handle_orphan_block(height);
    }
}
