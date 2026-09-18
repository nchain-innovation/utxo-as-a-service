use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::mpsc;

use std::time::Instant;

use chain_gang::messages::OutPoint;
use chain_gang::util::Hash256;

use crate::db::PooledConn;

use super::database::{height_from_sql, DBOperationType, MonitorRecord, SpendRecord, UtxoEntryDB};

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

/// An output being added to the spendable set, with everything the tables need.
pub struct NewOutput<'a> {
    pub hash: Hash256,
    pub index: usize,
    pub satoshis: i64,
    pub height: i32,
    pub locking_script: &'a [u8],
    /// The bytes the matching pattern captured, if it declared an identifier.
    pub identifier: Option<Vec<u8>>,
    /// Every monitor whose pattern selected this output.
    pub monitors: Vec<String>,
}

// provides access to utxo state and wraps interface to utxo table
pub struct Utxo {
    // Unspent tx
    utxo: HashMap<OutPoint, UtxoEntry>,
    // Database connection
    conn: PooledConn,

    // Record for batch write to utxo table
    utxo_entries: HashMap<OutPoint, UtxoEntryDB>,

    // Which monitors selected which outpoints, pending write.
    utxo_monitors: Vec<MonitorRecord>,

    // Spends seen but not yet mined, pending a move to utxo_spent.
    utxo_spends: Vec<SpendRecord>,

    // Spends seen in a block, grouped by the height that settles them.
    //
    // A BTreeMap rather than one Vec and a single height: `update_db` is called
    // per block today, so in practice there is one height, but `logic.rs` also
    // flushes on its own schedule and nothing in the type system says the two
    // cannot interleave. Keyed by height it cannot be wrong, and iterating a
    // BTreeMap applies the heights in order — a HashMap would make the order of
    // the emitted operations depend on hashing.
    utxo_settles: BTreeMap<i32, Vec<SpendRecord>>,

    // Outpoints we have recorded as spent but not yet settled.
    //
    // Needed because the settle must still find an outpoint whose mempool
    // sighting already took it out of the live set — that is the ordinary path
    // into a block, and skipping it there would leave the row unsettled for
    // ever. Entries leave on settle; the ones that never do are what CS-423
    // (mempool eviction) is about, and they leak here exactly as they leak in
    // the table.
    spent_unmined: HashSet<OutPoint>,

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
            utxo_monitors: Vec::new(),
            utxo_spends: Vec::new(),
            utxo_settles: BTreeMap::new(),
            spent_unmined: HashSet::new(),
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
            "SELECT txid, vout, satoshis, identifier, created_height, locking_script FROM utxo",
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
                    locking_script: row.get(5),
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

    /// An output a monitor selected.
    ///
    /// Grouped into a struct rather than passed as seven arguments: the
    /// positional call was already easy to get wrong with two `i32`-ish
    /// numbers next to each other, and this adds three more fields.
    pub fn add(&mut self, output: NewOutput<'_>) {
        let index_u32 = match output.index.try_into() {
            Ok(value) => value,
            Err(_) => {
                log::error!(
                    "UTXO output index {} out of range for tx {:?}",
                    output.index,
                    output.hash
                );
                return;
            }
        };

        // add a utxo outpoint, prepare a record to be written to database
        let outpoint = OutPoint {
            hash: output.hash,
            index: index_u32,
        };

        let new_entry = UtxoEntry {
            satoshis: output.satoshis,
            // lock_script: vout.lock_script.clone(),
            height: output.height,
            identifier: output.identifier.clone(),
        };
        // add to utxo list
        self.utxo.insert(outpoint.clone(), new_entry);

        // Record for batch write to utxo table
        let utxo_entry = UtxoEntryDB {
            hash: output.hash.0.to_vec(),
            pos: index_u32,
            satoshis: output.satoshis,
            height: output.height,
            locking_script: output.locking_script.to_vec(),
            identifier: output.identifier,
        };
        self.utxo_entries.insert(outpoint, utxo_entry);

        for monitor in output.monitors {
            self.utxo_monitors.push(MonitorRecord {
                txid: output.hash.0.to_vec(),
                vout: index_u32,
                monitor,
            });
        }
    }

    fn record(outpoint: &OutPoint, spending_txid: Hash256) -> SpendRecord {
        SpendRecord {
            txid: outpoint.hash.0.to_vec(),
            vout: outpoint.index,
            spending_txid: spending_txid.0.to_vec(),
        }
    }

    /// A spend seen but not yet mined.
    ///
    /// Guarded on the outpoint being one we track: once only monitored outputs
    /// are recorded, most prevouts the service sees belong to outputs it never
    /// held, and queueing a statement for each of those would be work
    /// proportional to the chain rather than to what is monitored.
    ///
    /// Note what this guard cannot do any more. It used to be the reason a
    /// second spend of an already-spent outpoint queued nothing, which
    /// `utxo03` documents. It is not a conflict check and must not be read as
    /// one: an absent outpoint is now overwhelmingly the ordinary case rather
    /// than a suspicious one. Detecting a conflicting spend is CS-428.
    pub fn spend(&mut self, outpoint: &OutPoint, spending_txid: Hash256) {
        if self.utxo.remove(outpoint).is_some() {
            self.utxo_entries.remove(outpoint);
            self.spent_unmined.insert(outpoint.clone());
            self.utxo_spends.push(Self::record(outpoint, spending_txid));
        }
    }

    /// A spend seen in a block, at `height`.
    ///
    /// Accepts an outpoint that is either still live or already recorded as
    /// spent-but-unmined. The second case is the ordinary route into a block —
    /// the spend was in the mempool first — and rejecting it here would leave
    /// the row with a NULL `spent_height` for ever.
    pub fn settle(&mut self, outpoint: &OutPoint, spending_txid: Hash256, height: i32) {
        let was_live = self.utxo.remove(outpoint).is_some();
        let was_unmined = self.spent_unmined.remove(outpoint);
        if was_live || was_unmined {
            self.utxo_entries.remove(outpoint);
            self.utxo_settles
                .entry(height)
                .or_default()
                .push(Self::record(outpoint, spending_txid));
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

        if !self.utxo_monitors.is_empty() {
            let monitors = std::mem::take(&mut self.utxo_monitors);
            self.send_db_op(DBOperationType::UtxoMonitorBatchWrite(monitors));
        }

        // Spends seen but not mined: move to utxo_spent, height still NULL.
        if !self.utxo_spends.is_empty() {
            let spends = std::mem::take(&mut self.utxo_spends);
            self.send_db_op(DBOperationType::UtxoBatchSpend(spends));
        }

        // Settles, in height order. One operation per height, because each is
        // a different pair of statements.
        for (height, spends) in std::mem::take(&mut self.utxo_settles) {
            self.send_db_op(DBOperationType::UtxoBatchSettle(spends, height));
        }
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
