use mysql::prelude::*;
use mysql::*;

use crate::config::Config;
use sv::messages::{Addr, Block, Headers, Tx};

use super::address_manager::AddressManager;
use super::block_manager::BlockManager;
use super::tx_analyser::TxAnalyser;

// Used to keep track of the server state
#[derive(Debug, PartialEq)]
pub enum ServerStateType {
    Starting,
    Disconnected,
    Connected,
    Ready,
}

impl ServerStateType {
    pub fn is_ready(&self) -> bool {
        // Return true if the server is in Ready state
        *self == ServerStateType::Ready
    }
}

// This captures the business logic associated with monitoring the blockchain
// it also provides a wrapper around the address_manager, block_manager and transaction_manager
pub struct Logic {
    state: ServerStateType,
    block_manager: BlockManager,
    tx_analyser: TxAnalyser,
    address_manager: AddressManager,
    // Used to keep track of the blocks downloaded, to determine if we need to download any more
    blocks_downloaded: usize,
    // Database connection
}

impl Logic {
    pub fn new(config: &Config) -> Self {
        let pool = Pool::new(&config.service.mysql_url).unwrap();
        let mut db_conn = pool.get_conn().unwrap();

        let block_conn = pool.get_conn().unwrap();
        let tx_conn = pool.get_conn().unwrap();
        let addr_conn = pool.get_conn().unwrap();
        Logic {
            state: ServerStateType::Starting,
            block_manager: BlockManager::new(config, block_conn),
            tx_analyser: TxAnalyser::new(config, tx_conn),
            address_manager: AddressManager::new(config, addr_conn),
            blocks_downloaded: 0,
        }
    }

    pub fn set_state(&mut self, state: ServerStateType) {
        println!("set state {:?}", &state);
        self.state = state;
    }

    pub fn on_headers(&self, headers: Headers) {
        println!("on_headers {:?}", headers);
    }

    pub fn on_block(&mut self, block: Block) {
        self.block_manager.add_block(block);

        if !self.state.is_ready() {
            self.blocks_downloaded += 1;
            if self.blocks_downloaded > 499 {
                // need to request a new inv
                self.blocks_downloaded = 0;
            }
        }
    }

    pub fn on_tx(&mut self, tx: Tx) {
        if self.state.is_ready() {
            self.tx_analyser.process_tx(tx);
        } else {
            // Queue up the tx for later processing
            self.tx_analyser.queue_tx(tx);
        }
    }

    pub fn on_addr(&mut self, addr: Addr) {
        self.address_manager.on_addr(addr);
    }
}
