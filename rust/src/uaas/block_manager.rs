use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::{Seek, SeekFrom},
    sync::mpsc,
    time::Instant,
};

use mysql::{prelude::*, PooledConn};

use chain_gang::{
    messages::{Block, BlockHeader, Payload},
    util::{Hash256, Serializable},
};

use crate::{
    config::Config,
    uaas::{
        database::{BlockHeaderWriteDB, DBOperationType, OrphanBlockHeaderWriteDB},
        tx_analyser::TxAnalyser,
        util::{delay_as_string, timestamp_age_as_sec, timestamp_as_string},
    },
};

// database header structure
struct DBHeader {
    height: u32,
    _hash: String,
    version: u32,
    prev_hash: String,
    merkle_root: String,
    timestamp: u32,
    bits: u32,
    nonce: u32,
    _position: u64,
    _blocksize: u32,
    _numtxs: u32,
}

// Used to record the block with a position in the block file
struct BlockWithPosition {
    pub position: Option<u64>,
    pub block: Block,
}

pub struct BlockManager {
    start_block_hash: String,
    // Startup read data from database or file
    startup_load_from_database: bool,

    block_file: String,
    save_blocks: bool,

    pub block_headers: Vec<BlockHeader>,
    pub hash_to_index: HashMap<Hash256, u32>,
    // BlockManager status
    // last block hash we processed
    last_hash_processed: Hash256,

    height: u32,

    // Queue of blocks that have arrived out of order - for later proceessing
    // we have changed to hashmap indexed by prev_hash for quicker processing
    // block_queue: Vec<Block>,
    block_queue: HashMap<Hash256, BlockWithPosition>,

    // Database connection
    conn: PooledConn,

    // Channel to database
    tx: mpsc::Sender<DBOperationType>,
    // orphan
    threshold: usize,
}

impl BlockManager {
    fn send_db_op(&self, op: DBOperationType) {
        if self.tx.send(op).is_err() {
            log::error!("Failed to send block database operation; channel closed");
        }
    }

    fn decode_stored_hash(label: &str, value: &str) -> Option<Hash256> {
        match Hash256::decode(value) {
            Ok(hash) => Some(hash),
            Err(err) => {
                log::error!("Invalid {label} hash {value}: {err:?}");
                None
            }
        }
    }

    pub fn new(
        config: &Config,
        conn: PooledConn,
        tx: mpsc::Sender<DBOperationType>,
    ) -> Result<Self, String> {
        let settings = config
            .get_network_settings()
            .map_err(|err| err.to_string())?;
        let start_block_hash = settings.start_block_hash.clone();
        let last_hash_processed = Hash256::decode(&start_block_hash)
            .map_err(|err| format!("Invalid start_block_hash '{start_block_hash}': {err:?}"))?;

        Ok(BlockManager {
            start_block_hash,
            startup_load_from_database: settings.startup_load_from_database,
            block_file: settings.block_file.clone(),
            save_blocks: settings.save_blocks,
            block_headers: Vec::new(),
            hash_to_index: HashMap::new(),
            height: settings.start_block_height + 1,
            last_hash_processed,
            block_queue: HashMap::new(),
            conn,
            tx,
            threshold: config.orphan.threshold,
        })
    }

    fn create_tables(&mut self) {
        // Create tables, if required
        // Check for the tables
        let tables: Vec<String> = match self.conn.query(
            "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_TYPE = 'BASE TABLE';",
        ) {
            Ok(tables) => tables,
            Err(err) => {
                log::error!("Unable to list database tables: {err:?}");
                return;
            }
        };

        if !tables.iter().any(|x| x.as_str() == "blocks") {
            log::info!("Table blocks not found - creating");
            if let Err(err) = self.conn.query_drop(
                r"CREATE TABLE blocks (
                    height int unsigned not null,
                    hash varchar(64) not null,
                    version int unsigned not null,
                    prev_hash varchar(64) not null,
                    merkle_root varchar(64) not null,
                    timestamp int unsigned not null,
                    bits int unsigned not null,
                    nonce int unsigned not null,
                    `offset` bigint unsigned not null,
                    blocksize int unsigned not null,
                    numtxs int unsigned not null,
                    CONSTRAINT PK_Entry PRIMARY KEY (hash));",
            ) {
                log::error!("Unable to create blocks table: {err:?}");
                return;
            }
            if let Err(err) = self
                .conn
                .query_drop(r"CREATE INDEX idx_hash ON blocks (hash);")
            {
                log::error!("Unable to create blocks hash index: {err:?}");
            }
        }

        if !tables.iter().any(|x| x.as_str() == "orphans") {
            log::info!("Table orphans not found - creating");
            if let Err(err) = self.conn.query_drop(
                r"CREATE TABLE orphans (
                    height int unsigned not null,
                    hash varchar(64) not null,
                    version int unsigned not null,
                    prev_hash varchar(64) not null,
                    merkle_root varchar(64) not null,
                    timestamp int unsigned not null,
                    bits int unsigned not null,
                    nonce int unsigned not null,
                    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP);",
            ) {
                log::error!("Unable to create orphans table: {err:?}");
            }
        }

        // Disable safe mode... wa ha ha - what could possibly go wrong?
        if let Err(err) = self.conn.query_drop("SET sql_safe_updates=0;") {
            log::warn!("Unable to disable sql_safe_updates: {err:?}");
        }
    }

    fn load_blockheaders_from_database(&mut self) {
        // load headers from database
        let start = Instant::now();

        let headers: Vec<DBHeader> = match self.conn.query_map(
            "SELECT * FROM blocks ORDER BY height asc",
            |(
                height,
                _hash,
                version,
                prev_hash,
                merkle_root,
                timestamp,
                bits,
                nonce,
                position,
                _blocksize,
                _numtxs,
            )| {
                DBHeader {
                    height,
                    _hash,
                    version,
                    prev_hash,
                    merkle_root,
                    timestamp,
                    bits,
                    nonce,
                    _position: position,
                    _blocksize,
                    _numtxs,
                }
            },
        ) {
            Ok(headers) => headers,
            Err(err) => {
                log::error!("Unable to load block headers from database: {err:?}");
                return;
            }
        };

        for b in headers {
            let Some(prev_hash) = Self::decode_stored_hash("prev_hash", &b.prev_hash) else {
                continue;
            };
            let Some(merkle_root) = Self::decode_stored_hash("merkle_root", &b.merkle_root) else {
                continue;
            };
            let block_header = BlockHeader {
                version: b.version,
                prev_hash,
                merkle_root,
                timestamp: b.timestamp,
                bits: b.bits,
                nonce: b.nonce,
            };
            // Store the block header
            let hash = block_header.hash();
            self.hash_to_index.insert(hash, b.height);
            self.block_headers.push(block_header);
            self.height = b.height + 1;
        }
        log::info!(
            "Loaded {} headers in {} seconds",
            self.block_headers.len(),
            start.elapsed().as_secs()
        );
    }

    fn process_block(&mut self, block: Block, tx_analyser: &mut TxAnalyser) {
        // Block processing functionality
        // This method is shared with reading from file and receiving blocks from network
        let hash = block.header.hash();
        log::info!(
            "process_block = {} {}",
            &hash.encode(),
            timestamp_as_string(block.header.timestamp)
        );

        // Determine if this block makes sense based on previous blocks
        // that is process them in chain order
        if self.last_hash_processed != block.header.prev_hash {
            log::error!(
                "Skipping out-of-order block {} (expected prev {}, got {})",
                hash.encode(),
                self.last_hash_processed.encode(),
                block.header.prev_hash.encode()
            );
            return;
        }
        self.last_hash_processed = hash;

        let block_height: i32 = match self.height.try_into() {
            Ok(height) => height,
            Err(err) => {
                log::error!(
                    "Block height {} is out of range for tx processing: {err}",
                    self.height
                );
                return;
            }
        };
        tx_analyser.process_block(&block, block_height);
        // Store the block header
        self.hash_to_index.insert(hash, self.height);
        self.block_headers.push(block.header);
        self.height += 1;
    }

    fn write_blockheader_to_database(
        &mut self,
        header: &BlockHeader,
        position: u64,
        blocksize: u32,
        numtxs: u32,
    ) {
        // Double check we haven't already written it
        if !self.hash_to_index.contains_key(&header.hash()) {
            // Write the block header to a database
            // Needs to be called before process block as process block increments the self.height
            let block_header = BlockHeaderWriteDB {
                height: self.height,
                hash: header.hash(),
                version: header.version,
                prev_hash: header.prev_hash,
                merkle_root: header.merkle_root,
                timestamp: header.timestamp,
                bits: header.bits,
                nonce: header.nonce,
                position,
                blocksize,
                numtxs,
            };

            self.send_db_op(DBOperationType::BlockHeaderWrite(block_header));
        }
    }

    fn delete_blockheader_from_database(&mut self, hash: &Hash256) {
        // Given the hash delete the associated blockheader from the blocks table
        self.send_db_op(DBOperationType::BlockHeaderDelete(*hash));
    }

    fn write_orphan_to_database(&mut self, header: &BlockHeader) {
        // Write the block header to a database
        // Needs to be called before process block as process block increments the self.height
        let block_header = OrphanBlockHeaderWriteDB {
            height: self.height,
            hash: header.hash(),
            version: header.version,
            prev_hash: header.prev_hash,
            merkle_root: header.merkle_root,
            timestamp: header.timestamp,
            bits: header.bits,
            nonce: header.nonce,
        };

        self.send_db_op(DBOperationType::OrphanBlockHeaderWrite(block_header));
    }

    fn process_block_queue(&mut self, tx_analyser: &mut TxAnalyser) {
        // Check block_queue to see if there are blocks that we can now process
        // loop through until last_hash_processed  == block.header.prev_hash
        // if found then check again

        // Remove block from block_queue
        while let Some(blockwithpos) = self.block_queue.remove(&self.last_hash_processed) {
            // do block processing

            let b = blockwithpos.block.clone();
            let blocksize = b.size() as u32;
            let numtxs = b.txns.len() as u32;

            // pos is either in the blockfile or we need to write to file
            let pos = match blockwithpos.position {
                Some(pos) => pos,
                None => self.write_block_to_file(&b),
            };
            // Write to database
            self.write_blockheader_to_database(&b.header, pos, blocksize, numtxs);
            self.process_block(b, tx_analyser);
        }
    }

    fn print_block_queue(&self) {
        if !self.block_queue.is_empty() {
            log::info!("self.block_queue.len() = {}", self.block_queue.len());
            if self.block_queue.len() < 5 {
                // print all block_queue entries
                for (_k, v) in self.block_queue.iter() {
                    log::info!(
                        "q_block = {} {}",
                        v.block.header.hash().encode(),
                        timestamp_as_string(v.block.header.timestamp)
                    );
                }
            }
        }
    }

    fn process_read_block(&mut self, block: Block, tx_analyser: &mut TxAnalyser, position: u64) {
        // Process each block as it is read from file
        let hash = block.header.hash();
        // Check to see if we already have this hash && blocks are in correct order
        if !self.hash_to_index.contains_key(&hash) {
            if self.last_hash_processed == block.header.prev_hash {
                let blocksize = block.size() as u32;
                let numtxs = block.txns.len() as u32;
                self.write_blockheader_to_database(&block.header, position, blocksize, numtxs);
                self.process_block(block, tx_analyser);

                // Check block_queue to see if there are blocks that we can now process
                self.process_block_queue(tx_analyser);
            } else {
                // Store block for later processing - if it is not already present
                if !self.block_queue.contains_key(&block.header.prev_hash) {
                    let prev_hash = block.header.prev_hash;
                    let entry = BlockWithPosition {
                        position: Some(position),
                        block,
                    };
                    self.block_queue.insert(prev_hash, entry);
                }
            }
        }
        self.print_block_queue();
    }

    fn read_blocks_from_file(&mut self, tx_analyser: &mut TxAnalyser) {
        // On loading check blocks are in the correct order and assert if not
        log::info!("read blocks");
        let start = Instant::now();

        // Read blocks from a file
        match OpenOptions::new().read(true).open(&self.block_file) {
            Ok(mut file) => {
                let mut position = match file.stream_position() {
                    Ok(pos) => pos,
                    Err(err) => {
                        log::warn!(
                            "Unable to read block file stream position for {}: {err}",
                            &self.block_file
                        );
                        0
                    }
                };
                while let Ok(block) = Block::read(&mut file) {
                    self.process_read_block(block, tx_analyser, position);
                    position = match file.stream_position() {
                        Ok(pos) => pos,
                        Err(err) => {
                            log::warn!(
                                "Unable to read block file stream position for {}: {err}",
                                &self.block_file
                            );
                            position
                        }
                    };
                }
            }
            Err(e) => log::info!("Unable to open block file {} - {}", &self.block_file, &e),
        }
        // Print blocks read
        let elapsed_time = start.elapsed().as_millis() as f64;
        log::info!(
            "{} blocks read in {} seconds",
            self.height,
            elapsed_time / 1000.0
        );
    }

    pub fn setup(&mut self, tx_analyser: &mut TxAnalyser) {
        // Does all the startup stuff a BlockManager needs to do
        self.create_tables();
        if self.startup_load_from_database {
            self.load_blockheaders_from_database();
            // Set the status - note that the height is updated by the load_blockheaders_from_database method
            if let Some(last_header) = self.block_headers.last() {
                self.last_hash_processed = last_header.hash();
            }
        } else {
            // Read in the blocks from the file
            self.read_blocks_from_file(tx_analyser);
        }
    }

    fn write_block_to_file(&mut self, block: &Block) -> u64 {
        // Write a block to a block file - should only be called for blocks received on network
        if !self.save_blocks {
            return 0;
        }

        let mut file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.block_file)
        {
            Ok(file) => file,
            Err(err) => {
                log::error!("Unable to open block file {}: {err}", self.block_file);
                return 0;
            }
        };
        let pos = match file.seek(SeekFrom::End(0)) {
            Ok(pos) => pos,
            Err(err) => {
                log::error!("Unable to seek block file {}: {err}", self.block_file);
                return 0;
            }
        };
        if let Err(err) = block.write(&mut file) {
            log::error!("Unable to write block to {}: {err}", self.block_file);
            return 0;
        }
        pos
    }

    pub fn handle_orphan_block(&mut self, tx_analyser: &mut TxAnalyser) {
        log::info!("Orphan block found! - handle_orphan_block");
        // Drop block queue - this will probably be empty anyway as we are probably on the tip, but just in case
        if !self.block_queue.is_empty() {
            log::info!("Clear block queue!");
            self.block_queue.clear();
        }
        // Drop the last block - from block headers
        if let Some(last_block) = self.block_headers.pop() {
            log::info!("Removing block {}", last_block.hash().encode());
            self.hash_to_index.remove(&last_block.hash());

            // Copy from blockheader from blocks to orphan table
            self.write_orphan_to_database(&last_block);
            self.delete_blockheader_from_database(&last_block.hash());
            // Remove tx at this block height
            tx_analyser.handle_orphan_block(self.height);

            // Reduce the block height
            self.height -= 1;
        }
    }

    // if there are more than n entries return the timestamp of the first
    // Used for detecting orphans
    pub fn get_start_block_timestamp(&self) -> Option<u32> {
        if self.block_headers.len() > self.threshold {
            // Just in case they are out of order for some reason we could get the smallest timestamp,
            // as this would be the earliest time
            self.block_headers.iter().map(|bh| bh.timestamp).min()
        } else {
            None
        }
    }

    pub fn on_block(&mut self, block: Block, tx_analyser: &mut TxAnalyser) {
        // On receiving block
        let start = Instant::now();

        // Handle block received on P2P network
        let hash = block.header.hash();

        // Check to see if we already have this hash - if so ignore it
        if !self.hash_to_index.contains_key(&hash) {
            // Check to see if block arrived in correct order
            if block.header.prev_hash == self.last_hash_processed {
                let pos = self.write_block_to_file(&block);
                // write to database
                let blocksize = block.size() as u32;
                let numtxs = block.txns.len() as u32;

                self.write_blockheader_to_database(&block.header, pos, blocksize, numtxs);
                // Note process_block increments the self.height
                self.process_block(block.clone(), tx_analyser);

                // Check block_queue to see if there are blocks that we can now process
                self.process_block_queue(tx_analyser);
            } else {
                // Store block for later processing - if it is not already present
                let prev_hash = block.header.prev_hash;
                if !self.block_queue.contains_key(&block.header.prev_hash) {
                    let entry = BlockWithPosition {
                        position: None,
                        block,
                    };
                    self.block_queue.insert(prev_hash, entry);
                }
            }
            self.print_block_queue();
        }
        let elapsed_time = start.elapsed().as_millis() as f64;
        log::info!("Block processing took {} seconds", elapsed_time / 1000.0);
    }

    pub fn get_last_known_block_hash(&self) -> String {
        // Return the last known block_hash as a String
        match self.block_headers.last() {
            None => self.start_block_hash.clone(),
            Some(header) => header.hash().encode(),
        }
    }

    pub fn has_chain_tip(&self) -> bool {
        // Return true if we have the chain tip
        // This is called after we receive a block
        if let Some(header) = self.block_headers.last() {
            let diff = timestamp_age_as_sec(header.timestamp);
            log::info!(
                "last header = {}, time behind tip = {}",
                header.hash().encode(),
                delay_as_string(diff)
            );

            // Assume chain tip if the block time is less than 10 mins ago
            // Note that we know all the predecessors are present in the list
            diff < 600
        } else {
            false
        }
    }
}
