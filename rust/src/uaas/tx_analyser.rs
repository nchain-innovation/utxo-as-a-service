use mysql::PooledConn;

use crate::config::Config;
use sv::messages::Tx;

pub struct TxAnalyser {
    // queue of pending txs
    tx_queue: Vec<Tx>,
}

impl TxAnalyser {
    pub fn new(_config: &Config, _conn: PooledConn) -> Self {
        TxAnalyser {
            tx_queue: Vec::new(),
        }
    }

    pub fn process_tx(&self, _tx: Tx) {}

    // Queue up the tx for later processing
    pub fn queue_tx(&mut self, tx: Tx) {
        self.tx_queue.push(tx);
    }
}
