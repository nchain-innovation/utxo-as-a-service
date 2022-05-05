use std::collections::HashMap;
use std::fs::OpenOptions;
use std::time::Instant;

use mysql::PooledConn;

use sv::messages::{Block, BlockHeader};
use sv::util::{Hash256, Serializable};

use crate::config::Config;
use crate::uaas::tx_analyser::TxAnalyser;
use crate::uaas::util::{timestamp_age_as_sec, timestamp_as_string};

pub struct BlockManager {
    start_block_hash: String,
    block_file: String,

    pub block_headers: Vec<BlockHeader>,
    pub hash_to_index: HashMap<Hash256, usize>,
    height: usize,
    // last block hash we processed
    last_hash_processed: Hash256,

    // Queue of blocks that have arrived out of order - for later proceessing
    block_queue: Vec<Block>,
}

impl BlockManager {
    pub fn new(config: &Config, _conn: PooledConn) -> Self {
        BlockManager {
            start_block_hash: config.service.start_block_hash.clone(),
            block_file: config.shared.block_file.clone(),
            block_headers: Vec::new(),
            hash_to_index: HashMap::new(),
            height: 0,
            last_hash_processed: Hash256::decode(&config.service.start_block_hash).unwrap(),
            block_queue: Vec::new(),
        }
    }

    fn write_block(&self, block: &Block) {
        // Write a block to a block file
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.block_file)
            .unwrap();
        block.write(&mut file).unwrap();
    }

    fn process_read_block(&mut self, block: Block, tx_analyser: &mut TxAnalyser) {
        // Process each block as it is read from file
        let hash = block.header.hash();
        // Check to see if we already have this hash
        if !self.hash_to_index.contains_key(&hash) {
            self.process_block(block, tx_analyser);
        }
    }

    pub fn read_blocks(&mut self, tx_analyser: &mut TxAnalyser) {
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
                // Store block for later processing
                self.block_queue.push(block);
            }

            if !self.block_queue.is_empty() {
                println!("self.block_queue.len() = {}", self.block_queue.len());

                // print all entries
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
            // Assume chain tip if
            // * the block time is less that 10 mins ago
            // Note that we know all the predecessors are present in the list
            diff < 600
        }
    }
}
