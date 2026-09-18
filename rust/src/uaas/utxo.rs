use std::collections::HashMap;
use std::sync::mpsc;

use std::time::Instant;

use chain_gang::messages::OutPoint;
use chain_gang::util::Hash256;

use crate::db::PooledConn;

use super::database::{height_from_sql, DBOperationType, UtxoEntryDB};

// Used to store the unspent txs (UTXO)
#[derive(Clone)]
pub struct UtxoEntry {
    satoshis: i64,
    // lock_script: Script, - have seen some very large script lengths here - removed for now
    height: i32, // use NOT_IN_BLOCK -1 to indicate that tx is not in block
    /// Bytes identifying what selected this output. `None` when the pattern
    /// declared no identifier.
    #[allow(dead_code)]
    identifier: Option<Vec<u8>>,
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
    fn send_db_op(&self, op: DBOperationType) {
        if self.tx.send(op).is_err() {
            log::error!("Failed to send utxo database operation; channel closed");
        }
    }

    /// Stored txids are raw `bytea` now, not 64-character hex, so this checks
    /// the length rather than parsing.
    fn decode_stored_hash(value: &[u8]) -> Option<Hash256> {
        match <[u8; 32]>::try_from(value) {
            Ok(bytes) => Some(Hash256(bytes)),
            Err(_) => {
                log::error!(
                    "Stored utxo txid is {} bytes, expected 32; row skipped",
                    value.len()
                );
                None
            }
        }
    }

    pub fn new(conn: PooledConn, tx: mpsc::Sender<DBOperationType>) -> Self {
        Utxo {
            utxo: HashMap::new(),
            conn,
            utxo_entries: HashMap::new(),
            utxo_deletes: Vec::new(),
            tx,
        }
    }

    pub fn load_utxo(&mut self) {
        // load outpoints from database
        let start = Instant::now();

        // Named columns rather than SELECT *: positional decoding breaks
        // silently when a migration adds a column, and the new table has more
        // of them than this reads.
        let rows = match self.conn.query(
            "SELECT txid, vout, satoshis, identifier, created_height FROM utxo",
            &[],
        ) {
            Ok(rows) => rows,
            Err(err) => {
                log::error!("Unable to load utxo from database: {err:?}");
                return;
            }
        };

        let txs: Vec<UtxoEntryDB> = rows
            .iter()
            .filter_map(|row| {
                let vout: i32 = row.get(1);
                // vout is non-negative by construction; a negative one would
                // mean the column had been written by something else.
                let Ok(pos) = u32::try_from(vout) else {
                    log::error!("Stored utxo vout {vout} is negative; row skipped");
                    return None;
                };
                Some(UtxoEntryDB {
                    hash: row.get(0),
                    pos,
                    satoshis: row.get(2),
                    identifier: row.get(3),
                    height: height_from_sql(row.get(4)),
                })
            })
            .collect();

        // Load entries into utxo struct
        for entry in txs {
            let Some(hash) = Self::decode_stored_hash(&entry.hash) else {
                continue;
            };

            let outpoint = OutPoint {
                hash,
                index: entry.pos,
            };
            let utxo_entry = UtxoEntry {
                satoshis: entry.satoshis,
                height: entry.height,
                identifier: entry.identifier,
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
        let index_u32 = match index.try_into() {
            Ok(value) => value,
            Err(_) => {
                log::error!("UTXO output index {index} out of range for tx {hash:?}");
                return;
            }
        };

        // add a utxo outpoint, prepare a record to be written to database
        let outpoint = OutPoint {
            hash,
            index: index_u32,
        };

        // `pubkeyhash` arrives as hex from script_to_pubkeyhash and the column
        // is now `bytea`. An empty string means the script was not p2pkh, which
        // is NULL rather than an empty identifier. CS-421 replaces this with
        // the matching pattern's named capture.
        let identifier = if pubkeyhash.is_empty() {
            None
        } else {
            match hex::decode(pubkeyhash) {
                Ok(bytes) => Some(bytes),
                Err(err) => {
                    log::error!("Unable to decode pubkeyhash {pubkeyhash}: {err:?}");
                    None
                }
            }
        };

        let new_entry = UtxoEntry {
            satoshis,
            // lock_script: vout.lock_script.clone(),
            height,
            identifier: identifier.clone(),
        };
        // add to utxo list
        self.utxo.insert(outpoint.clone(), new_entry);

        // Record for batch write to utxo table
        let utxo_entry = UtxoEntryDB {
            hash: hash.0.to_vec(),
            pos: index_u32,
            satoshis,
            height,
            identifier,
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
        self.send_db_op(DBOperationType::UtxoBatchWrite(request));
        self.utxo_entries.clear();

        // bulk/batch delete utxo table entries
        self.send_db_op(DBOperationType::UtxoBatchDelete(self.utxo_deletes.clone()));
        self.utxo_deletes.clear();
    }

    pub fn handle_orphan_block(&mut self, height: u32) {
        // Remove utxo of this block height
        self.send_db_op(DBOperationType::UtxoDelete(height));

        let Ok(height_as_i32) = i32::try_from(height) else {
            log::error!("Block height {height} out of range while pruning utxo set");
            return;
        };
        // Remove transactions at this height
        self.utxo
            .retain(|_outpoint, entry| entry.height != height_as_i32);
    }
}
