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
use super::util::timestamp_as_string;

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
    last_block_rx_time: Option<Instant>,
    need_to_request_blocks: bool,
}

impl Logic {
    pub fn new(config: &Config) -> Self {
        // Set up database connections for the componets
        let pool = Pool::new(&config.service.mysql_url).unwrap();

        let block_conn = pool.get_conn().unwrap();
        let tx_conn = pool.get_conn().unwrap();
        let addr_conn = pool.get_conn().unwrap();
        let mut logic = Logic {
            state: ServerStateType::Starting,
            tx_analyser: TxAnalyser::new(config, tx_conn),
            block_manager: BlockManager::new(config, block_conn),
            address_manager: AddressManager::new(config, addr_conn),
            blocks_downloaded: 0,
            last_block_rx_time: None,
            need_to_request_blocks: true,
        };
        logic.block_manager.read_blocks(&mut logic.tx_analyser);
        logic
    }

    pub fn set_state(&mut self, state: ServerStateType) {
        // Handles state changes

        assert_ne!(state, self.state);

        println!("set_state({:?})", &state);
        if state == ServerStateType::Ready {
            // Process blocks
            /*
            let start = Instant::now();
            for (height, block) in self.block_manager.blocks.iter().enumerate() {
                self.tx_analyser.process_block(block, height);
            }
            // Process queued transactions

            // Say how long it took
            let block_count = self.block_manager.block_headers.len();
            let elapsed_time = start.elapsed().as_millis() as f64;
            println!(
                "Processed {} blocks in {} seconds",
                block_count,
                elapsed_time / 1000.0
            );
            */
            // TODO: may want to check to see if tx still in mempool
            // should do this after every block anyway
        }
        self.state = state;
    }

    pub fn on_headers(&self, headers: Headers) {
        println!("on_headers {:?}", headers);
    }

    pub fn on_block(&mut self, block: Block) {
        // On rx Block
        self.last_block_rx_time = Some(Instant::now());

        // Print hash and timestamp
        println!(
            "{} - {}",
            block.header.hash().encode(),
            timestamp_as_string(block.header.timestamp)
        );

        self.block_manager.add_block(block);

        if !self.state.is_ready() {
            if self.block_manager.has_chain_tip() {
                self.set_state(ServerStateType::Ready);
                self.blocks_downloaded = 0;

            } else {
                self.blocks_downloaded += 1;
                if self.blocks_downloaded > 498 {
                    // need to request a new inv
                    self.need_to_request_blocks = true;
                }
            }
        }
    }

    pub fn on_tx(&mut self, tx: Tx) {
        // Handle TX message
        // Process straight away - goes to mempool
        self.tx_analyser.process_standalone_tx(&tx);
    }

    pub fn on_addr(&mut self, addr: Addr) {
        // Handle Addr message
        self.address_manager.on_addr(addr);
    }

    fn sufficient_time_elapsed(&self) -> bool {
        // Return true if sufficient time has passed since last block rx (if any)
        match self.last_block_rx_time {
            // More than 4 sec since last request
            Some(t) => t.elapsed().as_secs() > 2,
            None => true,
        }
    }

    fn need_to_request_blocks(&self) -> bool {
        // Return true if need to request a block
        if self.state.is_ready() {
            false
        } else {
            self.need_to_request_blocks && self.sufficient_time_elapsed()
        }
    }

    pub fn message_to_send(&mut self) -> Option<RequestMessage> {
        // Return a message to send, if any
        if self.need_to_request_blocks() {

            self.need_to_request_blocks = false;
            self.blocks_downloaded = 0;

            // Get the hash of the last known block
            let required_hash = self.block_manager.get_last_known_block_hash();
            println!("Requesting more blocks hash = {}", &required_hash);
            Some(RequestMessage::BlockRequest(required_hash))
        } else {
            None
        }
    }
}
