use std::collections::HashMap;

use mysql::PooledConn;

use sv::messages::{Block, OutPoint, Tx, TxIn, TxOut};
use sv::script::Script;
use sv::util::Hash256;

use crate::config::Config;

/*
    in - unlock_script - script sig
    out - lock_script - script public key

*/

/*
// Used to associate an address with a unlocking script and txs
pub struct P2PKH_Entry {
    address: String,
    unlock_script:  Script, // unlock_script == script pub key
    txs: Vec<Hash256>,
}
*/

// Used to store the unspent txs
pub struct UnspentEntry {
    satoshis: i64,
    lock_script: Script,
    height: Option<usize>,
}

// Used to store all txs
pub struct TxEntry {
    tx: Tx,
    height: Option<usize>,
}

pub struct TxAnalyser {
    // All transactions
    txs: HashMap<Hash256, TxEntry>,

    // Unspent tx
    unspent: HashMap<OutPoint, UnspentEntry>,
    // Address to script & tx mapping - can be replaced by collections
    // p2pkh_scripts: HashMap<String, P2PKH_Entry>,


}

impl TxAnalyser {
    pub fn new(_config: &Config, _conn: PooledConn) -> Self {
        TxAnalyser {
            txs: HashMap::new(),
            unspent: HashMap::new(),
            // p2pkh_scripts: HashMap::new(),
        }
    }
    /*
    fn is_p2pkh(&self, txin: &TxIn) -> bool {
        // Return true if vin is p2pkh
        txin.unlock_script.0.len() == 25
    }

    fn record_p2pkh(&self, hash: Hash256, txin: &TxIn) {
        // Given an input record p2pkh details
        let prev_hash = txin.prev_output.hash;
        if let Some(txentry) = self.txs.get(&prev_hash) {
            let prev_index: usize = txin.prev_output.index.try_into().unwrap();

            if let Some(vout) = txentry.tx.outputs.get(prev_index) {
                assert_eq!(vout.lock_script.0.len(), 25);

                // assert script_pub_key[:3].hex() == "76a914"
                assert_eq!(vout.lock_script.0[0..3], vec![0x76, 0xa9, 0x14] );

                // assert script_pub_key[-2:].hex() == "88ac"
                assert_eq!(vout.lock_script.0[..=2], vec![0x88, 0xac] );
                // extract the public key
            }
        }
        dbg!(&txin.unlock_script.0);

    }
    */
    fn is_spendable(&self, vout: &TxOut) -> bool {
        // Return true if the transaction output is spendable,
        // and therefore should go in the unspent outputs (UTXO) set.
        // OP_FALSE OP_RETURN (0x00, 0x61) is known to be unspendable.

        vout.lock_script.0[0..2] != vec![0x00, 0x6a]
    }

    pub fn process_tx(&mut self, tx: &Tx, height: Option<usize>, tx_index: Option<usize>) {
        // Process tx as we receive them,
        // note that we may see this tx as a standalone tx and then again in a block.

        let hash = tx.hash();

        // Store tx, if we haven't seen it already
        let tx_entry = TxEntry {
            tx: tx.clone(),
            height,
        };

        if let Some(_prev) = self.txs.insert(hash, tx_entry) {
            // We must have already processed this - skip the process inputs stage
            // Well if height is_some and we previously was is_none, then we need to update
            // self.unspent (if entry present)
        } else {
            // Process inputs - remove from unspent
            if height.is_some() && tx_index == Some(0) {
                // if is coinbase - nothing to process as these won't be in the unspent
            } else {
                for vin in tx.inputs.iter() {
                    // Remove from unspent
                    self.unspent.remove(&vin.prev_output);
                }
            }
        }

        // Collection processing - note maybe executed more than once..
        // TODO add here

        // Process outputs - add to unspent
        for (index, vout) in tx.outputs.iter().enumerate() {
            if self.is_spendable(vout) {
                let outpoint = OutPoint {
                    hash: hash,
                    index: index.try_into().unwrap(),
                };
                let entry = UnspentEntry {
                    satoshis: vout.satoshis,
                    lock_script: vout.lock_script.clone(),
                    height: height,
                };
                self.unspent.insert(outpoint, entry);
            }
        }
    }

    pub fn process_block(&mut self, block: &Block, height: usize) {
        // Given a block process all the tx in it
        for (tx_index, tx) in block.txns.iter().enumerate() {
            self.process_tx(tx, Some(height), Some(tx_index));
        }
    }


}
