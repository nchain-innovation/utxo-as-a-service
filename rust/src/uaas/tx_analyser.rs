use std::{cmp, sync::mpsc};

use crate::db::{Pool, PooledConn};

use chain_gang::{
    messages::{Block, Tx, TxOut},
    network::Network,
    util::Hash256,
};

use crate::{
    config::{CollectionConfig, Config},
    dynamic_config::DynamicConfig,
    uaas::{
        collection::{CollectionDatabase, WorkingCollection},
        database::DBOperationType,
        txdb::TxDB,
        utxo::{NewOutput, Utxo},
    },
};
/*
    in - unlock_script - script sig
    out - lock_script - script public key
*/

const NOT_IN_BLOCK: i32 = -1; // use -1 to indicate that this tx is not in block

pub struct TxAnalyser {
    save_txs: bool,
    // Database interface
    pub txdb: TxDB,
    // Unspent tx - make public so logic can write to database when in ready state
    pub utxo: Utxo,
    // Collections
    collection: Vec<WorkingCollection>,
    collection_db: CollectionDatabase,
    dynamic_config: DynamicConfig,
    network: Network,
}

impl TxAnalyser {
    fn pool_conn(pool: &Pool, label: &str) -> Result<PooledConn, String> {
        pool.get().map_err(|err| {
            log::error!("Unable to get {label} database connection: {err:?}");
            format!("Unable to get {label} database connection")
        })
    }

    pub fn new(
        config: &Config,
        pool: Pool,
        tx: mpsc::Sender<DBOperationType>,
    ) -> Result<Self, String> {
        let utxo_conn = Self::pool_conn(&pool, "utxo")?;
        let txdb_conn = Self::pool_conn(&pool, "txdb")?;
        let collection_conn = Self::pool_conn(&pool, "collection")?;

        let save_txs = config
            .get_network_settings()
            .map_err(|err| err.to_string())?
            .save_txs;
        let network = config.get_network().map_err(|err| err.to_string())?;
        let dynamic_config = DynamicConfig::new(config);
        let mut collection: Vec<WorkingCollection> = Vec::new();

        // Load the collections
        for c in &config.collection {
            match WorkingCollection::new(c.clone(), network) {
                Ok(wc) => collection.push(wc),
                Err(e) => log::error!("Error parsing collection {:?}", e),
            }
        }
        // load the dynamic collection
        for c in &dynamic_config.collection {
            match WorkingCollection::new(c.clone(), network) {
                Ok(wc) => collection.push(wc),
                Err(e) => log::error!("Error parsing collection {:?}", e),
            }
        }
        // Create a collection for broadcast txs
        let broadcast_collection: WorkingCollection =
            WorkingCollection::create_broadcast_collection();
        collection.push(broadcast_collection);

        Ok(TxAnalyser {
            save_txs,
            txdb: TxDB::new(txdb_conn, tx.clone(), save_txs),
            utxo: Utxo::new(utxo_conn, tx),
            collection,
            collection_db: CollectionDatabase::new(collection_conn, config),
            dynamic_config: dynamic_config.clone(),
            network,
        })
    }

    fn read_tables(&mut self) {
        // Load datastructures from the database tables
        self.txdb.load_mempool();
        if self.save_txs {
            self.txdb.load_tx();
        }

        self.utxo.load_utxo();
        // Load Collections
        for c in self.collection.iter_mut() {
            c.txs = self.collection_db.load_txs(c.name());
        }
    }

    pub fn setup(&mut self) {
        // Do the startup setup that is required for tx analyser
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

    /// Every monitor whose pattern selects this locking script, in
    /// configuration order.
    ///
    /// Order matters twice: it decides which pattern's capture becomes the
    /// output's identifier when several match, and it is what makes that choice
    /// reproducible. `collection` is a Vec built from the config in order, so
    /// the same script always yields the same identifier.
    fn monitors_for(&self, script: &[u8]) -> Vec<String> {
        self.collection
            .iter()
            .filter(|c| c.matches_script(script))
            .map(|c| c.name().to_string())
            .collect()
    }

    /// The identifier the first matching pattern captured, if any declared one.
    fn identifier_for(&self, script: &[u8]) -> Option<Vec<u8>> {
        self.collection
            .iter()
            .find_map(|c| c.identifier_in(script))
            .map(<[u8]>::to_vec)
    }

    /// Records the spendable outputs of this transaction that a monitor
    /// selected.
    ///
    /// **This is the change in what the service means.** It used to record
    /// every spendable output of every transaction it saw — the whole UTXO set
    /// of the chain. It now records only what a configured pattern selects, so
    /// the table is what is being watched rather than everything that exists.
    ///
    /// Two consequences worth naming:
    ///
    /// * a prevout absent from the set no longer suggests anything is wrong. It
    ///   is the ordinary case for almost every transaction, which is why
    ///   conflict detection cannot be built on absence (CS-428);
    /// * keeping the full locking script becomes affordable, because the volume
    ///   is what the patterns select rather than the chain.
    fn process_tx_outputs(&mut self, tx: &Tx, height: i32) {
        let hash = tx.hash();
        for (index, vout) in tx.outputs.iter().enumerate() {
            if !self.is_spendable(vout) {
                continue;
            }
            let script = &vout.lock_script.0;
            let monitors = self.monitors_for(script);
            if monitors.is_empty() {
                continue;
            }
            self.utxo.add(NewOutput {
                hash,
                index,
                satoshis: vout.satoshis,
                height,
                locking_script: script,
                identifier: self.identifier_for(script),
                monitors,
            });
        }
    }

    /// Records the spends this transaction makes.
    ///
    /// `height` is `NOT_IN_BLOCK` for a transaction seen in the mempool and the
    /// block height otherwise, and that is the whole difference: an unmined
    /// spend moves the outpoint to `utxo_spent` with a NULL height, a mined one
    /// settles it. A spend is no longer a delete — the row moves rather than
    /// disappearing, so the spend is still there to be reported after it
    /// happens.
    fn process_tx_inputs(&mut self, tx: &Tx, height: i32, blockindex: usize) {
        if blockindex == 0 {
            // if is coinbase (blockindex 0)- nothing to process as these won't be in the utxo
            return;
        }
        let spending_txid = tx.hash();
        for vin in tx.inputs.iter() {
            if height == NOT_IN_BLOCK {
                self.utxo.spend(&vin.prev_output, spending_txid);
            } else {
                self.utxo.settle(&vin.prev_output, spending_txid, height);
            }
        }
    }

    fn process_collection(&mut self, tx: &Tx, is_uaas_broadcast_tx: bool) {
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
        // write to a broadcast collection - if hasn't already been picked up by previous collections
        if is_uaas_broadcast_tx {
            // get broadcast_collection
            match self.collection.iter_mut().find(|c| c.name() == "broadcast") {
                Some(broadcast_collection) => {
                    // write to a broadcast collection - if hasn't already been picked up by previous collections
                    broadcast_collection.push(tx.hash());
                    self.collection_db
                        .write_tx_to_database(broadcast_collection.name(), tx);
                }
                None => {
                    log::warn!("Unable to find broadcast collection");
                }
            };
        }
    }

    pub fn process_block_tx(&mut self, tx: &Tx, height: i32, blockindex: usize) {
        // Process tx as received in a block from a peer

        // process inputs
        self.process_tx_inputs(tx, height, blockindex);

        // Process outputs
        // Note this will overwrite the utxo outpoints with height = NOT_IN_BLOCK(-1)
        // and utxo entries
        self.process_tx_outputs(tx, height);

        // Collection processing
        self.process_collection(tx, false);
    }

    pub fn process_block(&mut self, block: &Block, height: i32) {
        // Given a block process all the txs in it

        self.txdb.process_block(block, height);

        // now process Txs...
        for (blockindex, tx) in block.txns.iter().enumerate() {
            self.process_block_tx(tx, height, blockindex);
        }

        // Do db writes here
        self.flush_database_cache();
    }

    pub fn flush_database_cache(&mut self) {
        self.utxo.update_db();
        self.txdb.batch_delete_from_mempool();
        self.txdb.batch_write_mempool();
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

    pub fn process_standalone_tx(&mut self, tx: &Tx, is_uaas_broadcast_tx: bool) {
        // Process standalone tx as we receive them.
        // Note standalone tx are txs that are not in a block.
        let fee = self.calc_fee(tx);

        self.txdb.add_to_mempool(tx, fee);

        // Process inputs
        const NOT_A_COINBASE_TX: usize = 1;

        self.process_tx_inputs(tx, NOT_IN_BLOCK, NOT_A_COINBASE_TX);

        // Process outputs
        self.process_tx_outputs(tx, NOT_IN_BLOCK);

        // Collection processing
        self.process_collection(tx, is_uaas_broadcast_tx);

        self.txdb.batch_write_mempool();
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
        log::info!("add_monitor {:?}", monitor);
        // Check name is not in collection
        if !self.is_name_in_collection(&monitor.name) {
            // add to collection
            match WorkingCollection::new(monitor.clone(), self.network) {
                Ok(wc) => {
                    self.collection.push(wc);
                    // add to dynamic config
                    self.dynamic_config.add(&monitor);
                }
                Err(e) => log::error!("Error parsing collection {:?}", e),
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
                None => log::error!("Error indexing collection {}", monitor_name),
            }
            // Delete from dynamic config
            self.dynamic_config.delete(monitor_name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tests::sample_config;
    use crate::uaas::database::SpendRecord;
    use crate::uaas::database::{MonitorRecord, UtxoEntryDB};
    use chain_gang::messages::{Block, OutPoint, TxIn, TxOut};
    use chain_gang::script::Script;
    use std::sync::mpsc::{self, Receiver, TryRecvError};

    // A 25-byte p2pkh locking script. `marker` fills the pubkeyhash so that
    // otherwise-identical fixture transactions hash to distinct txids.
    fn p2pkh_script(marker: u8) -> Script {
        let mut script = Vec::with_capacity(25);
        script.extend_from_slice(&[0x76, 0xa9, 0x14]);
        script.extend_from_slice(&[marker; 20]);
        script.extend_from_slice(&[0x88, 0xac]);
        Script(script)
    }

    fn funding_tx(marker: u8) -> Tx {
        Tx {
            version: 1,
            inputs: vec![TxIn::default()],
            outputs: vec![TxOut {
                satoshis: 1_000,
                lock_script: p2pkh_script(marker),
            }],
            lock_time: 0,
        }
    }

    fn spending_tx(prev_output: OutPoint) -> Tx {
        Tx {
            version: 1,
            inputs: vec![TxIn {
                prev_output,
                ..TxIn::default()
            }],
            outputs: vec![TxOut {
                satoshis: 900,
                lock_script: p2pkh_script(0xee),
            }],
            lock_time: 0,
        }
    }

    // TxAnalyser::new needs four pooled connections, so these tests need a
    // reachable server. Same convention as the schema and rest_api tests:
    // skip when UAAS_TEST_POSTGRES_URL is unset rather than fail.
    fn analyser_with_live_db(test_name: &str) -> Option<(TxAnalyser, Receiver<DBOperationType>)> {
        let Ok(url) = std::env::var("UAAS_TEST_POSTGRES_URL") else {
            eprintln!("skipping {test_name}: UAAS_TEST_POSTGRES_URL not set");
            return None;
        };
        let pool = crate::db::build_pool(&url).expect("connect to UAAS_TEST_POSTGRES_URL");
        let (tx, rx) = mpsc::channel();
        let analyser = TxAnalyser::new(&sample_config(), pool, tx).expect("construct TxAnalyser");
        Some((analyser, rx))
    }

    fn drain(rx: &Receiver<DBOperationType>) -> Vec<DBOperationType> {
        let mut ops = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(op) => ops.push(op),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return ops,
            }
        }
    }

    /// Every settle record a run of operations carries, with its height.
    fn settles(ops: Vec<DBOperationType>) -> Vec<(SpendRecord, i32)> {
        ops.into_iter()
            .filter_map(|op| match op {
                DBOperationType::UtxoBatchSettle(spends, height) => Some((spends, height)),
                _ => None,
            })
            .flat_map(|(spends, height)| spends.into_iter().map(move |s| (s, height)))
            .collect()
    }

    fn is_outpoint(record: &SpendRecord, outpoint: &OutPoint) -> bool {
        record.txid == outpoint.hash.0.to_vec() && record.vout == outpoint.index
    }

    #[test]
    fn utxo01_spending_a_block_tx_moves_the_outpoint_and_queues_the_settle() {
        let Some((mut analyser, rx)) = analyser_with_live_db(
            "utxo01_spending_a_block_tx_moves_the_outpoint_and_queues_the_settle",
        ) else {
            return;
        };

        // blockindex 0 => treated as coinbase, inputs are not processed.
        let funding = funding_tx(0x11);
        analyser.process_block_tx(&funding, 100, 0);

        let outpoint = OutPoint {
            hash: funding.hash(),
            index: 0,
        };
        assert_eq!(
            analyser.utxo.get_satoshis(&outpoint),
            Some(1_000),
            "funding output should be in the utxo set"
        );

        let spender = spending_tx(outpoint.clone());
        analyser.process_block_tx(&spender, 101, 1);

        assert_eq!(
            analyser.utxo.get_satoshis(&outpoint),
            None,
            "spent outpoint should have left the in-memory spendable set"
        );

        analyser.utxo.update_db();
        let settled = settles(drain(&rx));

        // A settle, not a delete: the row moves to utxo_spent carrying the
        // height that mined it and the txid that spent it, so the spend is
        // still reportable afterwards.
        let record = settled
            .iter()
            .find(|(record, _)| is_outpoint(record, &outpoint))
            .unwrap_or_else(|| {
                panic!("spent outpoint should have been queued to settle, got {settled:?}")
            });
        assert_eq!(record.1, 101, "settled at the height that mined the spend");
        assert_eq!(
            record.0.spending_txid,
            spender.hash().0.to_vec(),
            "the spending txid is recorded"
        );
    }

    // Probe L (CS-405) — a malleated sibling is indistinguishable from a
    // second, unrelated transaction.
    //
    // Two announcements spend the same prevout and pay the same outputs; only
    // the unlocking script differs, so the txids differ. That is transaction
    // malleability: pre-confirmation the txid is not a stable identity.
    //
    // Nothing here detects it. The mempool is keyed on txid alone
    // (`txdb.rs`, `HashMap<Hash256, Hash256>`), there is no prevout index
    // anywhere, and `process_tx_inputs` deletes the prevout without asking
    // whether it was already spent. A malleated sibling and a genuine
    // double-spend are equally invisible.
    //
    // This probe could not be written before CS-393: with the delete path dead
    // there was nothing to observe. Now that deletes execute, the observable
    // consequence is below — the UTXO set gains two live entries for what is
    // economically one output.
    //
    // TODO(UAAS-18): provisional identity derived from the prevout and output
    // sets, with the txid provisional until a block pins it. When that lands,
    // the second announcement must be recognised as a conflicting spend rather
    // than silently doubling the balance.
    fn malleated_pair(prev_output: OutPoint) -> (Tx, Tx) {
        let outputs = vec![TxOut {
            satoshis: 900,
            lock_script: p2pkh_script(0xcc),
        }];
        // version 2: nothing in the crate branches on version, which is the
        // premise of review question 4.
        let build = |unlock: &[u8]| Tx {
            version: 2,
            inputs: vec![TxIn {
                prev_output: prev_output.clone(),
                unlock_script: Script(unlock.to_vec()),
                ..TxIn::default()
            }],
            outputs: outputs.clone(),
            lock_time: 0,
        };
        // Same spend, two encodings of the unlocking script. OP_NOP is a
        // no-op, so this is malleation in the strict sense: the scripts are
        // behaviourally identical.
        (build(&[0x51]), build(&[0x51, 0x61]))
    }

    #[test]
    fn utxo03_a_malleated_sibling_doubles_the_utxo_set_today() {
        let Some((mut analyser, rx)) =
            analyser_with_live_db("utxo03_a_malleated_sibling_doubles_the_utxo_set_today")
        else {
            return;
        };

        let funding = funding_tx(0x31);
        analyser.process_block_tx(&funding, 300, 0);
        let outpoint = OutPoint {
            hash: funding.hash(),
            index: 0,
        };
        assert_eq!(analyser.utxo.get_satoshis(&outpoint), Some(1_000));

        let (tx_a, tx_b) = malleated_pair(outpoint.clone());

        // The pair really is a malleated pair, not two different spends.
        assert_ne!(tx_a.hash(), tx_b.hash(), "the txids must differ");
        assert_eq!(
            tx_a.inputs[0].prev_output, tx_b.inputs[0].prev_output,
            "both must spend the same prevout"
        );
        assert_eq!(tx_a.outputs, tx_b.outputs, "both must pay the same outputs");
        assert_eq!(tx_a.version, tx_b.version);

        analyser.process_block_tx(&tx_a, 301, 1);
        assert_eq!(
            analyser.utxo.get_satoshis(&outpoint),
            None,
            "the first spend removes the funding outpoint"
        );

        analyser.process_block_tx(&tx_b, 301, 2);

        // The finding. One output was funded and one output was spent, but the
        // utxo set now carries both siblings' outputs as live and unrelated.
        let out_a = OutPoint {
            hash: tx_a.hash(),
            index: 0,
        };
        let out_b = OutPoint {
            hash: tx_b.hash(),
            index: 0,
        };
        assert_eq!(
            (
                analyser.utxo.get_satoshis(&out_a),
                analyser.utxo.get_satoshis(&out_b)
            ),
            (Some(900), Some(900)),
            "documents the phantom balance: 900 satoshis counted twice from 1000 funded"
        );

        // And the conflict still leaves no trace. `Utxo::settle` records a
        // spend only for an outpoint that was live or awaiting settlement; by
        // the time the sibling arrives the first spend has taken it out of
        // both, so the second queues nothing at all — not even a duplicate for
        // an operator to notice.
        //
        // CS-421 changed the vocabulary here from delete to settle and changed
        // nothing about the finding. Detecting the conflict is CS-428.
        analyser.utxo.update_db();
        let settled = settles(drain(&rx));
        assert_eq!(
            settled
                .iter()
                .filter(|(record, _)| is_outpoint(record, &outpoint))
                .count(),
            1,
            "the second spend of the same outpoint is silently discarded"
        );
    }

    /// Writes a run of operations carries.
    fn writes(ops: Vec<DBOperationType>) -> Vec<UtxoEntryDB> {
        ops.into_iter()
            .filter_map(|op| match op {
                DBOperationType::UtxoBatchWrite(entries) => Some(entries),
                _ => None,
            })
            .flatten()
            .collect()
    }

    fn monitor_rows(ops: Vec<DBOperationType>) -> Vec<MonitorRecord> {
        ops.into_iter()
            .filter_map(|op| match op {
                DBOperationType::UtxoMonitorBatchWrite(records) => Some(records),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// A transaction paying one output to an arbitrary script.
    fn tx_paying(script: Script) -> Tx {
        Tx {
            version: 1,
            inputs: Vec::new(),
            outputs: vec![TxOut {
                satoshis: 1_000,
                lock_script: script,
            }],
            lock_time: 0,
        }
    }

    // The filter. Only what a pattern selects is recorded now, so a spendable
    // output no monitor matched must leave no trace at all.
    #[test]
    fn utxo05_an_unmonitored_output_is_not_recorded() {
        let Some((mut analyser, rx)) =
            analyser_with_live_db("utxo05_an_unmonitored_output_is_not_recorded")
        else {
            return;
        };

        // OP_TRUE: spendable, and matched by none of sample_config's patterns.
        let ignored = tx_paying(Script(vec![0x51]));
        // A p2pkh output, which the `fixtures` monitor does select — present so
        // this test cannot pass merely because nothing is ever recorded.
        let watched = tx_paying(p2pkh_script(0x51));

        analyser.process_block_tx(&ignored, 400, 1);
        analyser.process_block_tx(&watched, 400, 2);

        let ignored_out = OutPoint {
            hash: ignored.hash(),
            index: 0,
        };
        let watched_out = OutPoint {
            hash: watched.hash(),
            index: 0,
        };
        assert_eq!(
            analyser.utxo.get_satoshis(&ignored_out),
            None,
            "an output no monitor selected must not enter the spendable set"
        );
        assert_eq!(
            analyser.utxo.get_satoshis(&watched_out),
            Some(1_000),
            "a monitored output must still be recorded"
        );

        analyser.utxo.update_db();
        let written = writes(drain(&rx));
        assert!(
            written
                .iter()
                .all(|entry| entry.hash != ignored.hash().0.to_vec()),
            "nothing should be queued for the unmonitored output"
        );
        assert!(
            written
                .iter()
                .any(|entry| entry.hash == watched.hash().0.to_vec()),
            "the monitored output should be queued"
        );
    }

    // The identifier is the pattern's named capture, in raw bytes. The old
    // stopgap produced a 40-character hex string of the same hash; 20 bytes is
    // what distinguishes the capture from it.
    #[test]
    fn utxo06_the_identifier_is_the_patterns_captured_bytes() {
        let Some((mut analyser, rx)) =
            analyser_with_live_db("utxo06_the_identifier_is_the_patterns_captured_bytes")
        else {
            return;
        };

        let watched = tx_paying(p2pkh_script(0x7e));
        analyser.process_block_tx(&watched, 401, 1);
        analyser.utxo.update_db();

        let written = writes(drain(&rx));
        let entry = written
            .iter()
            .find(|entry| entry.hash == watched.hash().0.to_vec())
            .expect("the monitored output is recorded");

        assert_eq!(
            entry.identifier.as_deref(),
            Some(&[0x7e; 20][..]),
            "the identifier is the 20 captured bytes, not 40 hex characters"
        );
        assert_eq!(
            entry.locking_script,
            p2pkh_script(0x7e).0,
            "the full locking script is carried, not an empty placeholder"
        );
    }

    // utxo_monitor is the many-to-many record of which patterns selected an
    // outpoint. sample_config has two collections with patterns; only
    // `fixtures` matches an arbitrary p2pkh script, so exactly one row.
    #[test]
    fn utxo07_utxo_monitor_records_the_monitors_that_matched() {
        let Some((mut analyser, rx)) =
            analyser_with_live_db("utxo07_utxo_monitor_records_the_monitors_that_matched")
        else {
            return;
        };

        let watched = tx_paying(p2pkh_script(0x6d));
        analyser.process_block_tx(&watched, 402, 1);
        analyser.utxo.update_db();

        let rows = monitor_rows(drain(&rx));
        let mine: Vec<&MonitorRecord> = rows
            .iter()
            .filter(|r| r.txid == watched.hash().0.to_vec() && r.vout == 0)
            .collect();

        assert_eq!(
            mine.iter().map(|r| r.monitor.as_str()).collect::<Vec<_>>(),
            vec!["fixtures"],
            "every monitor whose pattern selected the output is recorded"
        );
    }

    // CS-421 evidence, not a design test.
    //
    // CS-421's acceptance criteria say utxo03's assertion is inverted because
    // the settle is keyed on the outpoint. This test is the reason that is not
    // so. The two things CS-421 introduces on the output path are a filter on
    // the locking script and an identifier captured from it — and a malleated
    // pair pays byte-identical outputs, so neither can tell the siblings apart.
    // Whatever the filter admits for one, it admits for the other.
    //
    // Keying the settle on the outpoint fixes a real and different bug: a spend
    // seen in the mempool as A and mined as its sibling B settles, where a
    // settle keyed on the spending txid matches nothing and leaves spent_height
    // NULL forever. That is pinned separately, against the database.
    //
    // The doubling needs conflict detection — TODO(UAAS-18)'s provisional
    // identity over the prevout and output sets — and is out of scope here.
    #[test]
    fn utxo04_monitored_output_filtering_cannot_separate_a_malleated_pair() {
        let prev_output = OutPoint {
            hash: funding_tx(0x41).hash(),
            index: 0,
        };
        let (tx_a, tx_b) = malleated_pair(prev_output);

        // The premise: different transactions, identical outputs.
        assert_ne!(tx_a.hash(), tx_b.hash(), "the txids must differ");
        assert_eq!(tx_a.outputs, tx_b.outputs, "both must pay the same outputs");

        // The filter CS-421 adds, over the script the pair actually pays.
        let matcher = crate::uaas::hex_pattern::ScriptMatcher::compile(
            "76a914(?<identifier>[0-9a-f]{40})88ac",
        )
        .expect("pattern compiles");

        let script_a = &tx_a.outputs[0].lock_script.0;
        let script_b = &tx_b.outputs[0].lock_script.0;

        assert_eq!(
            matcher.is_match(script_a),
            matcher.is_match(script_b),
            "a filter on the locking script cannot distinguish malleated siblings"
        );
        assert!(
            matcher.is_match(script_a),
            "this fixture must actually be selected, or the test proves nothing"
        );
        assert_eq!(
            matcher.identifier(script_a),
            matcher.identifier(script_b),
            "the captured identifier is the same for both siblings"
        );

        // So both siblings' outputs are admitted, and the phantom balance
        // utxo03 documents survives monitored-output filtering unchanged.
    }

    #[test]
    fn utxo02_process_block_visits_every_transaction_and_records_the_height() {
        let Some((mut analyser, rx)) = analyser_with_live_db(
            "utxo02_process_block_visits_every_transaction_and_records_the_height",
        ) else {
            return;
        };

        const HEIGHT: i32 = 200;
        let txns: Vec<Tx> = (0..5u8).map(|i| funding_tx(0x20 + i)).collect();
        let block = Block {
            txns: txns.clone(),
            ..Block::default()
        };

        analyser.process_block(&block, HEIGHT);

        for (blockindex, tx) in txns.iter().enumerate() {
            let outpoint = OutPoint {
                hash: tx.hash(),
                index: 0,
            };
            assert_eq!(
                analyser.utxo.get_satoshis(&outpoint),
                Some(1_000),
                "output of tx at blockindex {blockindex} should be in the utxo set"
            );
        }

        // process_block flushes the cache, so the write batch is already on the
        // channel. Every entry must carry the block height, not NOT_IN_BLOCK.
        let written: Vec<UtxoEntryDB> = drain(&rx)
            .into_iter()
            .filter_map(|op| match op {
                DBOperationType::UtxoBatchWrite(entries) => Some(entries),
                _ => None,
            })
            .flatten()
            .collect();

        assert_eq!(
            written.len(),
            txns.len(),
            "every transaction in the block should have contributed a utxo row"
        );
        for entry in &written {
            assert_eq!(
                entry.height,
                HEIGHT,
                "utxo row {} should be recorded at the block height, not {}",
                hex::encode(&entry.hash),
                NOT_IN_BLOCK
            );
        }
    }
}
