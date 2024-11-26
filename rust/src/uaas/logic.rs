use std::{sync::mpsc, thread};

use mysql::Pool;

use chain_gang::{
    messages::{Addr, Block, BlockLocator, Headers, Inv, InvVect, Message, Tx},
    util::Hash256,
};

use crate::{
    config::Config,
    uaas::{
        address_manager::AddressManager, block_manager::BlockManager, connection::Connection,
        database::Database, tx_analyser::TxAnalyser,
    },
};

// Constants for inv messages
const TX: u32 = 1;
const BLOCK: u32 = 2;

// Used to keep track of the server state
#[derive(Debug, PartialEq, Eq)]
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
    pub tx_analyser: TxAnalyser,
    address_manager: AddressManager,
    pub connection: Connection,

    //database: Database,
    thread: Option<thread::JoinHandle<()>>,

    // Orphan detection
    detecting_orphans: bool,
    start_block_timestamp: Option<u32>,
    // For sending message to peer
    send_message_queue: Vec<Message>,
    // Record the block inv messages received
    block_inventory: Vec<Vec<InvVect>>,
}

impl Logic {
    pub fn new(config: &Config) -> Self {
        // Set up database connections for the components
        let pool = Pool::new(config.get_mysql_url())
            .expect("Problem connecting to database. Check database is connected and database connection configuration is correct.\n");

        let block_conn = pool.get_conn().unwrap();
        let addr_conn = pool.get_conn().unwrap();
        let connection_conn = pool.get_conn().unwrap();
        let db_conn = pool.get_conn().unwrap();

        // Channel for database writes
        let (tx, rx) = mpsc::channel();

        let mut logic = Logic {
            state: ServerStateType::Starting,
            tx_analyser: TxAnalyser::new(config, pool, tx.clone()),
            block_manager: BlockManager::new(config, block_conn, tx),
            address_manager: AddressManager::new(config, addr_conn),
            connection: Connection::new(config, connection_conn),

            //database:
            thread: None,
            // orphans
            detecting_orphans: config.orphan.detect,
            start_block_timestamp: None,

            // For sending message to peer
            send_message_queue: Vec::new(),
            // For record of the block inv messages received
            block_inventory: Vec::new(),
        };

        let db_config = config.clone();
        logic.thread = Some(thread::spawn(move || {
            let mut database = Database::new(db_conn, rx, &db_config);
            database.perform_db_operations();
        }));

        logic
    }

    pub fn setup(&mut self) {
        // Do any start up component setup required
        self.address_manager.setup();
        self.tx_analyser.setup();
        self.block_manager.setup(&mut self.tx_analyser);
        self.connection.setup();
    }

    pub fn set_state(&mut self, state: ServerStateType) {
        // Handles state changes
        log::info!("set_state({:?})", &state);
        if state == ServerStateType::Connected {
            // Reset the request time on connection/reconnection
            self.request_next_block(None);
        }
        self.state = state;
    }

    pub fn on_headers(&self, headers: Headers) {
        log::info!("on_headers {:?}", headers);
    }

    // Return true if this is an orphan block
    fn is_orphan(&mut self, timestamp: u32) -> bool {
        // Are we detecting orphans and is the block before our first block
        if self.detecting_orphans {
            match self.start_block_timestamp {
                Some(start_block_timestamp) => start_block_timestamp > timestamp,
                None => {
                    // If we dont already have the timestamp, request it
                    match self.block_manager.get_start_block_timestamp() {
                        Some(start_block_timestamp) => {
                            // Record timestamp here
                            self.start_block_timestamp = Some(start_block_timestamp);
                            start_block_timestamp > timestamp
                        }
                        None => false,
                    }
                }
            }
        } else {
            false
        }
    }

    pub fn on_tx(&mut self, tx: Tx, is_uaas_broadcast_tx: bool) {
        // Handle TX message,
        // Process straight away - goes to mempool
        self.tx_analyser
            .process_standalone_tx(&tx, is_uaas_broadcast_tx);
        if self.state.is_ready() {
            // if we are in ready state write utxo out
            self.tx_analyser.utxo.update_db();
        }
    }

    pub fn flush_database_cache(&mut self) {
        if self.state.is_ready() {
            // if we are in ready state write utxo out
            self.tx_analyser.flush_database_cache()
        }
    }

    pub fn tx_exists(&self, hash: Hash256) -> bool {
        self.tx_analyser.tx_exists(hash)
    }

    pub fn on_addr(&mut self, addr: Addr) {
        // Handle Addr message
        self.address_manager.on_addr(addr);
    }

    // Return a list of messages to send
    pub fn message_to_send(&mut self) -> Vec<Message> {
        let mut msg_q: Vec<Message> = Vec::new();
        // move any inv messages over to msg_q
        msg_q.append(&mut self.send_message_queue);
        msg_q
    }

    pub fn on_inv(&mut self, inv: Inv) {
        // Inv message handling logic

        let txs: Vec<InvVect> = inv
            .objects
            .clone()
            .into_iter()
            .filter(|x| x.obj_type == TX)
            .collect();
        // Request all txs
        if !txs.is_empty() {
            let want = Message::GetData(Inv { objects: txs });
            self.send_message_queue.push(want);
        }

        let blocks: Vec<InvVect> = inv
            .objects
            .into_iter()
            .filter(|x| x.obj_type == BLOCK)
            .collect();

        let is_empty = self.block_inventory.is_empty();
        // Add to block_inventory
        self.block_inventory.push(blocks);
        // if the list was empty request the first entry
        if is_empty {
            self.request_next_block(None);
        }
    }

    fn get_last_known_block_hash(&mut self) -> String {
        if cfg!(feature = "rnd_orphans") {
            // approx 75% of the time
            let perc_chance = rand::random::<u8>() > 64;
            if perc_chance {
                self.block_manager.get_last_known_block_hash()
            } else {
                log::info!("orphan time");
                "000000000003fc68ed563be8e3d8b5e6b211392ac266e4be5a416ec74fbe25aa".to_string()
            }
        } else {
            self.block_manager.get_last_known_block_hash()
        }
    }

    fn request_next_block(&mut self, hash: Option<Hash256>) {
        // Remove the received hash from the inventory
        log::info!("request_next_block {:?}", &hash);
        if let Some(hash) = hash {
            // no point looking if there is nothing in the block_inventory
            if !self.block_inventory.is_empty() {
                // As each hash arrives remove it from the block_inventory
                self.block_inventory[0].retain(|block| block.hash != hash);
            }
        }
        // while there is an empty entry at the front of block_inventory
        while !self.block_inventory.is_empty() && self.block_inventory[0].is_empty() {
            // remove empty list from front
            let _ = self.block_inventory.remove(0);
        }

        // Display block_inventory
        if !self.block_inventory.is_empty() {
            log::info!(
                "block_inventory.len = {}, [0].len = {}",
                self.block_inventory.len(),
                self.block_inventory[0].len()
            );
        } else {
            log::info!(
                "self.block_inventory.is_empty() = {}",
                self.block_inventory.is_empty()
            );
        }

        // if no block_inventory left, we need to request more with GetBlocks
        if self.block_inventory.is_empty() {
            let hash = self.get_last_known_block_hash();
            log::info!("Requesting more blocks from hash = {}", &hash);

            // Build getblocks message - this results in an inv message
            let mut locator = BlockLocator::default();
            let hash = Hash256::decode(&hash).unwrap();
            locator.block_locator_hashes.push(hash);
            let message = Message::GetBlocks(locator);
            self.send_message_queue.push(message)

            // else take the first block off the queue
        } else if let Some(block) = self.block_inventory[0].first() {
            let object: Vec<InvVect> = vec![block.clone()];
            // Request the block with GetData
            log::info!("requesting GetData {:?}", object[0].hash.encode());
            let want = Message::GetData(Inv { objects: object });
            self.send_message_queue.push(want);
        } else {
            // really shouldn't get here
            log::info!("wrong place");
            self.block_inventory
                .iter()
                .enumerate()
                .for_each(|(i, list)| log::info!("list {}, len = {}", i, list.len()));
            panic!("should not get here");
            // block_inv not empty but nothing in first entry
        }
    }

    pub fn on_block(&mut self, block: Block) {
        // On rx Block
        let block_hash: Option<Hash256> = if self.is_orphan(block.header.timestamp) {
            // Forget the blocks that we are going to request
            self.block_inventory.clear();
            // Ignore this block and remove previous block from block_manager
            self.block_manager
                .handle_orphan_block(&mut self.tx_analyser);
            log::info!("Orphan block found! - ignore!");
            None
        } else {
            let hash = block.header.hash();
            // Call the block manager
            self.block_manager.on_block(block, &mut self.tx_analyser);
            Some(hash)
        };
        // Request next block or if hash is None request inv
        self.request_next_block(block_hash);

        // Determine if has caught up with chain tip
        if !self.state.is_ready() && self.block_manager.has_chain_tip() {
            self.set_state(ServerStateType::Ready);
        }
    }
}
