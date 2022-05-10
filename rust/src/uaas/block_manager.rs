use std::collections::HashMap;
use std::fs::OpenOptions;
use std::time::Instant;

use mysql::prelude::*;
use mysql::PooledConn;
use mysql::*;

use sv::messages::{Block, BlockHeader};
use sv::util::{Hash256, Serializable};

use crate::config::Config;
use crate::uaas::tx_analyser::TxAnalyser;
use crate::uaas::util::{timestamp_age_as_sec, timestamp_as_string};

#[derive(Debug)]
// database header structure
struct DBHeader {
    _height: u32,
    _hash: String,
    version: u32,
    prev_hash: String,
    merkle_root: String,
    timestamp: u32,
    bits: u32,
    nonce: u32,
}

pub struct BlockManager {
    start_block_hash: String,
    block_file: String,

    pub block_headers: Vec<BlockHeader>,
    pub hash_to_index: HashMap<Hash256, usize>,
    // BlockManager status
    // last block hash we processed
    last_hash_processed: Hash256,
    last_height_processed: usize,

    height: usize,

    // Queue of blocks that have arrived out of order - for later proceessing
    block_queue: Vec<Block>,

    // Database connection
    conn: PooledConn,
}

impl BlockManager {
    pub fn new(config: &Config, conn: PooledConn) -> Self {
        BlockManager {
            start_block_hash: config.service.start_block_hash.clone(),
            block_file: config.shared.block_file.clone(),
            block_headers: Vec::new(),
            hash_to_index: HashMap::new(),
            height: 0,
            last_hash_processed: Hash256::decode(&config.service.start_block_hash).unwrap(),
            last_height_processed: 0,
            block_queue: Vec::new(),
            conn,
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
            self.conn
                .query_drop(
                    r"CREATE TABLE blocks (
                    height int unsigned,
                    hash text,
                    version int unsigned,
                    prev_hash text,
                    merkle_root text,
                    timestamp int unsigned,
                    bits int unsigned,
                    nonce int unsigned
                );",
                )
                .unwrap();
        }
    }

    fn load_headers(&mut self) {
        // load headers from database
        let start = Instant::now();

        let headers = self
            .conn
            .query_map(
                "SELECT * FROM blocks ORDER BY height",
                |(_height, _hash, version, prev_hash, merkle_root, timestamp, bits, nonce)| {
                    DBHeader {
                        _height,
                        _hash,
                        version,
                        prev_hash,
                        merkle_root,
                        timestamp,
                        bits,
                        nonce,
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
            self.hash_to_index.insert(hash, self.height);
            self.block_headers.push(block_header);
            self.height += 1;
        }
        println!(
            "Loaded {} headers in {} seconds",
            self.block_headers.len(),
            start.elapsed().as_secs()
        );
    }

    pub fn setup(&mut self, _tx_analyser: &mut TxAnalyser) {
        // Does all the startup stuff a BlockManager needs to do
        self.create_tables();
        self.load_headers();
        // Set the status
        if let Some(last_header) = self.block_headers.last() {
            self.last_hash_processed = last_header.hash();
        }
        self.last_height_processed = self.block_headers.len();

        // we no longer read in the blocks from the file
        // self.read_blocks(tx_analyser);
    }

    fn process_block(&mut self, block: Block, tx_analyser: &mut TxAnalyser) {
        // Block processing functionality
        // This method is shared with reading from file and receiving blocks
        let hash = block.header.hash();

        // Determine if this block makes sense based on previous blocks
        // that is process them in chain order
        assert_eq!(self.last_hash_processed, block.header.prev_hash);
        self.last_hash_processed = hash;

        tx_analyser.process_block(&block, self.height);
        // Store the block header
        self.hash_to_index.insert(hash, self.height);
        self.block_headers.push(block.header);
        self.height += 1;
    }

    fn process_read_block(&mut self, block: Block, tx_analyser: &mut TxAnalyser) {
        // Process each block as it is read from file
        let hash = block.header.hash();
        // Check to see if we already have this hash
        if !self.hash_to_index.contains_key(&hash) {
            // Only process the blocks that we havent already processed
            if self.height <= self.last_height_processed {
                self.height += 1;
            } else {
                self.process_block(block, tx_analyser);
            }
        }
    }

    fn read_blocks(&mut self, tx_analyser: &mut TxAnalyser) {
        // On loading check blocks are in the correct order and assert if not
        println!("read blocks");
        let start = Instant::now();

        // Read blocks from a file
        match OpenOptions::new().read(true).open(&self.block_file) {
            Ok(mut file) => {
                // Success - read blocks
                while let Ok(block) = Block::read(&mut file) {
                    self.process_read_block(block, tx_analyser);
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

    fn store_block(&mut self, header: &BlockHeader) {
        // Write the block header to a database
        //.query_drop(r"CREATE TABLE blocks (
        //    height int, hash text, version int, prev_hash text, merkle_root text, timestamp int, bits int, nonce int)")
        let index = self.height - 1;

        let blocks_insert = format!(
            "INSERT INTO blocks
            VALUES ({}, '{}', {}, '{}', '{}', {}, {}, {});",
            index,
            header.hash().encode(),
            header.version,
            header.prev_hash.encode(),
            header.merkle_root.encode(),
            header.timestamp,
            header.bits,
            header.nonce
        );
        self.conn.exec_drop(&blocks_insert, Params::Empty).unwrap();
    }

    fn write_block(&mut self, block: &Block) {
        // Write a block to a block file
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.block_file)
            .unwrap();

        block.write(&mut file).unwrap();

        // write to database
        self.store_block(&block.header);
    }

    fn process_block_queue(&mut self, tx_analyser: &mut TxAnalyser) {
        // Check block_queue to see if there are blocks that we can now process
        // loop through until last_hash_processed  == block.header.prev_hash
        // if found then check again
        while let Some(block) = self
            .block_queue
            .iter_mut()
            .find(|block| block.header.prev_hash == self.last_hash_processed)
        {
            let prev_hash = self.last_hash_processed;
            // do block processing
            let b = block.clone();
            self.write_block(&b);
            self.process_block(b, tx_analyser);

            // Remove block from list
            self.block_queue
                .retain(|block| block.header.prev_hash != prev_hash);
        }
    }

    pub fn on_block(&mut self, block: Block, tx_analyser: &mut TxAnalyser) {
        // Handle block received on P2P network
        let hash = block.header.hash();

        // Check to see if we already have this hash - if so ignore it
        if !self.hash_to_index.contains_key(&hash) {
            // check to see if block arrived in correct order
            if block.header.prev_hash == self.last_hash_processed {
                self.process_block(block.clone(), tx_analyser);
                self.write_block(&block);

                // Check block_queue to see if there are blocks that we can now process
                self.process_block_queue(tx_analyser);
            } else {
                // Store block for later processing - if it is not already present
                if self.block_queue.iter().all(|b| b.header.hash() != hash) {
                    self.block_queue.push(block);
                }
            }

            if !self.block_queue.is_empty() {
                println!("self.block_queue.len() = {}", self.block_queue.len());
                if self.block_queue.len() < 5 {
                    // print all block_queue entries
                    for b in self.block_queue.iter() {
                        println!(
                            "q_block = {} {}",
                            b.header.hash().encode(),
                            timestamp_as_string(b.header.timestamp)
                        );
                    }
                }
            }
        }
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
            // Assume chain tip if the block time is less than 10 mins ago
            // Note that we know all the predecessors are present in the list
            diff < 600
        }
    }
}
