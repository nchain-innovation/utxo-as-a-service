use std::time::Instant;
// use mysql::prelude::*;
//use mysql::*;
use mysql::Pool;

use sv::messages::{Addr, Block, Headers, Tx};

use crate::config::Config;
use crate::event_handler::RequestMessage;

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
    last_block_request_time: Option<Instant>,
}

impl Logic {
    pub fn new(config: &Config) -> Self {
        // Set up database connections for the componets
        let pool = Pool::new(&config.service.mysql_url).unwrap();

        let block_conn = pool.get_conn().unwrap();
        let tx_conn = pool.get_conn().unwrap();
        let addr_conn = pool.get_conn().unwrap();
        Logic {
            state: ServerStateType::Starting,
            block_manager: BlockManager::new(config, block_conn),
            tx_analyser: TxAnalyser::new(config, tx_conn),
            address_manager: AddressManager::new(config, addr_conn),
            blocks_downloaded: 0,
            last_block_request_time: None,
        }
    }

    pub fn set_state(&mut self, state: ServerStateType) {
        println!("set state {:?}", &state);
        if state == ServerStateType::Ready {
            // do stuff
        }
        self.state = state;
    }

    pub fn on_headers(&self, headers: Headers) {
        println!("on_headers {:?}", headers);
    }

    pub fn on_block(&mut self, block: Block) {
        self.block_manager.add_block(block);

        if !self.state.is_ready() {
            if self.block_manager.has_chain_tip() {
                self.set_state(ServerStateType::Ready)

            } else {
                self.blocks_downloaded += 1;
                // TODO: rethink this
                if self.blocks_downloaded > 499 {
                    // need to request a new inv
                    self.blocks_downloaded = 0;
                }
            }
        }
    }

    pub fn on_tx(&mut self, tx: Tx) {
        // Handle TX message
        if self.state.is_ready() {
            self.tx_analyser.process_tx(tx);
        } else {
            // Queue up the tx for later processing
            self.tx_analyser.queue_tx(tx);
        }
    }

    pub fn on_addr(&mut self, addr: Addr) {
        // Handle Addr message
        self.address_manager.on_addr(addr);
    }

    fn sufficient_time_elapsed(&self) -> bool {
        // Return true if sufficient time has passed since last request (if any)
        match self.last_block_request_time {
            // more than 4 sec since last request
            Some(t) => t.elapsed().as_secs() > 4,
            None => true,
        }
    }

    fn need_to_request_blocks(&self) -> bool {
        // Return true if need to request a block
        if self.state.is_ready() {
            false
        } else {
            match self.blocks_downloaded {
                0 => self.sufficient_time_elapsed(),
                499 => self.sufficient_time_elapsed(),
                _ => false,
            }
        }
    }

    pub fn message_to_send(&mut self) -> Option<RequestMessage> {
        // Return a message to send, if any
        if self.need_to_request_blocks() {
            // Update last request time
            self.last_block_request_time = Some(Instant::now());

            let required_hash = self.block_manager.get_last_known_block_hash();
            dbg!(&required_hash);
            Some(RequestMessage::BlockRequest(required_hash))
        } else {
            None
        }
    }
}
