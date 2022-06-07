use std::time::Instant;

use mysql::Pool;

use sv::messages::{Addr, Block, Headers, Tx};

use crate::config::Config;
use crate::event_handler::RequestMessage;

use super::address_manager::AddressManager;
use super::block_manager::BlockManager;
use super::connection::Connection;
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
    pub connection: Connection,
    // Used to keep track of the blocks downloaded, to determine if we need to download any more
    blocks_downloaded: usize,
    last_block_rx_time: Option<Instant>,
    need_to_request_blocks: bool,

    // Used to determine the time between requesting blocks
    block_request_period: u64,
}

impl Logic {
    pub fn new(config: &Config) -> Self {
        // Set up database connections for the componets
        let pool = Pool::new(&config.get_mysql_url())
            .expect("Problem connecting to database. Check database is connected and database connection configuration is correct.\n");

        let block_conn = pool.get_conn().unwrap();
        let tx_conn = pool.get_conn().unwrap();
        let addr_conn = pool.get_conn().unwrap();
        let connection_conn = pool.get_conn().unwrap();
        Logic {
            state: ServerStateType::Starting,
            tx_analyser: TxAnalyser::new(config, tx_conn),
            block_manager: BlockManager::new(config, block_conn),
            address_manager: AddressManager::new(config, addr_conn),
            connection: Connection::new(config, connection_conn),
            blocks_downloaded: 0,
            last_block_rx_time: None,
            need_to_request_blocks: true,
            block_request_period: config.get_network_settings().block_request_period,
        }
    }

    pub fn setup(&mut self) {
        // Do any start up component setup required
        self.address_manager.setup();

        self.tx_analyser.setup();
        self.block_manager.setup(&mut self.tx_analyser);
        self.connection.setup();
        // Reset the request time
        // self.last_block_rx_time = Some(Instant::now());
    }

    pub fn set_state(&mut self, state: ServerStateType) {
        // Handles state changes
        // Ensure that the state changes
        assert_ne!(state, self.state);
        println!("set_state({:?})", &state);
        self.state = state;
    }

    pub fn on_headers(&self, headers: Headers) {
        println!("on_headers {:?}", headers);
    }

    pub fn on_block(&mut self, block: Block) {
        // On rx Block
        self.last_block_rx_time = Some(Instant::now());

        // Call the block manager
        self.block_manager.on_block(block, &mut self.tx_analyser);

        if !self.state.is_ready() {
            if self.block_manager.has_chain_tip() {
                self.set_state(ServerStateType::Ready);
                self.blocks_downloaded = 0;
                self.need_to_request_blocks = false;
            } else {
                self.blocks_downloaded += 1;
                if self.blocks_downloaded > 499 {
                    // need to request more blocks
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
            // More than x sec since last block
            Some(t) => t.elapsed().as_secs() > self.block_request_period,
            None => true,
        }
    }

    fn need_to_request_blocks(&self) -> bool {
        // Return true if need to request a block
        if self.state.is_ready() {
            false
        } else {
            self.need_to_request_blocks || self.sufficient_time_elapsed()
        }
    }

    pub fn message_to_send(&mut self) -> Option<RequestMessage> {
        // Return a message to send tp request blocks, if any
        if !self.state.is_ready() {
            // no debug info once in ready mode
            dbg!(self.blocks_downloaded);
            // dbg!(self.need_to_request_blocks);
        }
        if self.need_to_request_blocks() {
            self.blocks_downloaded = 0;
            self.need_to_request_blocks = false;
            // Reset the request time
            self.last_block_rx_time = Some(Instant::now());

            // Get the hash of the last known block
            let required_hash = self.block_manager.get_last_known_block_hash();
            println!("Requesting more blocks from hash = {}", &required_hash);
            Some(RequestMessage::BlockRequest(required_hash))
        } else {
            None
        }
    }
}
