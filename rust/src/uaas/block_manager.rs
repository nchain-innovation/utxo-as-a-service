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
        util::{timestamp_age_as_sec, timestamp_as_string},
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
}

impl BlockManager {
    pub fn new(config: &Config, conn: PooledConn, tx: mpsc::Sender<DBOperationType>) -> Self {
        BlockManager {
            start_block_hash: config.get_network_settings().start_block_hash.clone(),
            startup_load_from_database: config.get_network_settings().startup_load_from_database,
            block_file: config.get_network_settings().block_file.clone(),
            save_blocks: config.get_network_settings().save_blocks,
            block_headers: Vec::new(),
            hash_to_index: HashMap::new(),
            height: config.get_network_settings().start_block_height + 1,
            last_hash_processed: Hash256::decode(&config.get_network_settings().start_block_hash)
                .unwrap(),
            block_queue: HashMap::new(),
            conn,
            tx,
        }
    }

    fn create_tables(&mut self) {
        // Create tables, if required
        // Check for the tables
        let tables: Vec<String> = self
            .conn
            .query(
                "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_TYPE = 'BASE TABLE';",
            )
            .unwrap();

        if !tables.iter().any(|x| x.as_str() == "blocks") {
            println!("Table blocks not found - creating");
            self.conn
                .query_drop(
                    r"CREATE TABLE blocks (
                    height int unsigned not null,
                    hash varchar(64) not null,
                    version int unsigned not null,
                    prev_hash varchar(64) not null,
                    merkle_root varchar(64) not null,
                    timestamp int unsigned not null,
                    bits int unsigned not null,
                    nonce int unsigned not null,
                    offset bigint unsigned not null,
                    blocksize int unsigned not null,
                    numtxs int unsigned not null,
                    CONSTRAINT PK_Entry PRIMARY KEY (hash));",
                )
                .unwrap();
            self.conn
                .query_drop(r"CREATE INDEX idx_hash ON blocks (hash);")
                .unwrap();
        }

        if !tables.iter().any(|x| x.as_str() == "orphans") {
            println!("Table orphans not found - creating");
            self.conn
                .query_drop(
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
                )
                .unwrap();
        }
    }

    fn load_blockheaders_from_database(&mut self) {
        // load headers from database
        let start = Instant::now();

        let headers = self
            .conn
            .query_map(
                "SELECT * FROM blocks ORDER BY height",
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
            )
            .unwrap();

        for b in headers {
            let block_header = BlockHeader {
                version: b.version,
                prev_hash: Hash256::decode(&b.prev_hash).unwrap(),
                merkle_root: Hash256::decode(&b.merkle_root).unwrap(),
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
        println!(
            "Loaded {} headers in {} seconds",
            self.block_headers.len(),
            start.elapsed().as_secs()
        );
    }

    fn process_block(&mut self, block: Block, tx_analyser: &mut TxAnalyser) {
        // Block processing functionality
        // This method is shared with reading from file and receiving blocks from network
        let hash = block.header.hash();
        println!(
            "process_block = {} {}",
            &hash.encode(),
            timestamp_as_string(block.header.timestamp)
        );

        // Determine if this block makes sense based on previous blocks
        // that is process them in chain order
        assert_eq!(self.last_hash_processed, block.header.prev_hash);
        self.last_hash_processed = hash;

        // try_into().unwrap() is required to convert u32 -> i32
        tx_analyser.process_block(&block, self.height.try_into().unwrap());
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

        self.tx
            .send(DBOperationType::BlockHeaderWrite(block_header))
            .unwrap();
    }

    fn delete_blockheader_from_database(&mut self, hash: &Hash256) {
        // Given the hash delete the associated blockheader from the blocks table
        self.tx
            .send(DBOperationType::BlockHeaderDelete(*hash))
            .unwrap();
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

        self.tx
            .send(DBOperationType::OrphanBlockHeaderWrite(block_header))
            .unwrap();
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
            println!("self.block_queue.len() = {}", self.block_queue.len());
            if self.block_queue.len() < 5 {
                // print all block_queue entries
                for (_k, v) in self.block_queue.iter() {
                    println!(
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
        println!("read blocks");
        let start = Instant::now();

        // Read blocks from a file
        match OpenOptions::new().read(true).open(&self.block_file) {
            Ok(mut file) => {
                let mut position = file.stream_position().unwrap();
                // Success - read blocks
                while let Ok(block) = Block::read(&mut file) {
                    self.process_read_block(block, tx_analyser, position);
                    position = file.stream_position().unwrap();
                }
            }
            Err(e) => println!("Unable to open block file {} - {}", &self.block_file, &e),
        }
        // Print blocks read
        let elapsed_time = start.elapsed().as_millis() as f64;
        println!(
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
        if self.save_blocks {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.block_file)
                .unwrap();
            let pos = file.seek(SeekFrom::End(0)).unwrap();
            block.write(&mut file).unwrap();
            pos
        } else {
            0 // always give an offset of 0 if nothing is
        }
    }

    pub fn handle_orphan_block(&mut self) {
        println!("Orphan block found!");
        // Drop block queue - this will probably be empty anyway as we are probably on the tip, but just in case
        if !self.block_queue.is_empty() {
            println!("Clear block queue!");
            self.block_queue.clear();
        }
        // Drop the last block - from block headers
        if !self.block_headers.is_empty() {
            // Remove the block header
            let last_block = self.block_headers.pop().unwrap();
            println!("Removing block {}", last_block.hash().encode());
            self.hash_to_index.remove(&last_block.hash());
            self.height -= 1;

            // Copy from blockheader from blocks to orphan table
            self.write_orphan_to_database(&last_block);
            self.delete_blockheader_from_database(&last_block.hash());
        }
    }

    // if there are more than 5 entries return the timestamp of the first
    pub fn get_start_block_timestamp(&self) -> Option<u32> {
        if self.block_headers.len() > 5 {
            self.block_headers.first().map(|bh| bh.timestamp)
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
        println!("Block processing took {} seconds", elapsed_time / 1000.0);
    }

    pub fn get_last_known_block_hash(&self) -> String {
        // Return the last known block_hash as a String
        if self.block_headers.is_empty() {
            self.start_block_hash.clone()
        } else {
            // Now we know the list is in order we can just return the last entry's hash
            let header = self.block_headers.last().unwrap();
            header.hash().encode()
        }
    }

    pub fn has_chain_tip(&self) -> bool {
        // Return true if we have the chain tip
        // This is called after we receive a block

        if self.block_headers.is_empty() {
            false
        } else {
            let diff = timestamp_age_as_sec(self.block_headers.last().unwrap().timestamp);
            let header = self.block_headers.last().unwrap();
            println!("last header = {}", header.hash().encode());
            println!("has_chain_tip - diff = {}", &diff);
            // Assume chain tip if the block time is less than 10 mins ago
            // Note that we know all the predecessors are present in the list
            diff < 600
        }
    }
}
