use std::collections::HashMap;
use std::fs::OpenOptions;
use std::time::{SystemTime, UNIX_EPOCH};

use mysql::PooledConn;

use sv::messages::Block;
use sv::util::{Hash256, Serializable};

use crate::config::Config;

pub struct BlockManager {
    start_block_hash: String,
    block_file: String,
    pub blocks: Vec<Block>,
}

impl BlockManager {
    pub fn new(config: &Config, _conn: PooledConn) -> Self {
        let mut b = BlockManager {
            start_block_hash: config.service.start_block_hash.clone(),
            block_file: config.shared.block_file.clone(),
            blocks: Vec::new(),
        };
        b.read_blocks();
        b.sort_blocks();
        b
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

    fn read_blocks(&mut self) {
        // Read blocks from a file
        match OpenOptions::new().read(true).open(&self.block_file) {
            Ok(mut file) => {
                // Success - read blocks
                while let Ok(block) = Block::read(&mut file) {
                    // dbg!(&block);
                    self.blocks.push(block);
                }
            }
            Err(e) => println!("Unable to open block file {} - {}", &self.block_file, &e),
        }
        println!("{} blocks read", self.blocks.len());
    }

    fn sort_blocks(&mut self) {
        // Sort the blocks, initally by timestamp (smallest first)
        self.blocks
            .sort_by(|a, b| a.header.timestamp.cmp(&b.header.timestamp))
    }

    pub fn add_block(&mut self, block: Block) {
        let hash = block.header.hash();
        // Check to see if we already have this hash
        let found = self.blocks.iter().find(|x| x.header.hash() == hash);
        if found.is_none() {
            self.write_block(&block);
            // this just appends to the end of the list
            // TODO: figure out if want to be cleverer
            self.blocks.push(block);
        }
    }

    // pub fn on_block(&self, _block: Block) {}

    pub fn get_last_known_block_hash(&self) -> String {
        // Return the last known block_hash as a String
        if self.blocks.is_empty() {
            self.start_block_hash.clone()
        } else {
            // work through list of blocks and return the last hash that links
            // start with the known block hash
            let mut last_hash = Hash256::decode(self.start_block_hash.as_str()).unwrap();
            // Mapping to speed up lookups
            let prev_to_hash: HashMap<Hash256, Hash256> = self
                .blocks
                .iter()
                .map(|b| (b.header.prev_hash, b.header.hash()))
                .collect();

            // While successfully finding the next hash
            while let Some(hash) = prev_to_hash.get(&last_hash) {
                last_hash = *hash;
            }

            last_hash.encode()
        }
    }

    fn have_all_predecessors(&self, hash: Hash256) -> bool {
        // return true if have all predecessors of block
        // Mapping to speed up lookups
        let hash_to_prev: HashMap<Hash256, Hash256> = self
            .blocks
            .iter()
            .map(|b| (b.header.hash(), b.header.prev_hash))
            .collect();
        let last_hash = Hash256::decode(self.start_block_hash.as_str()).unwrap();
        let mut good_hash = hash;

        // While successfully finding the next hash
        while let Some(hash) = hash_to_prev.get(&good_hash) {
            good_hash = *hash;
        }
        // check to see if we got to the root hash
        good_hash == last_hash
    }

    pub fn has_chain_tip(&self) -> bool {
        // Return true if we have the chain tip
        // This is called after we receive a block

        if self.blocks.is_empty() {
            false
        } else {
            let block_timestamp: u64 = self.blocks.last().unwrap().header.timestamp.into();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let diff = if now > block_timestamp {
                now - block_timestamp
            } else {
                0
            };

            // Assume chain tip if
            // * the block time is less that 10 mins ago
            if diff < 600 {
                // * and we have all predecessors
                let block_prev_hash = self.blocks.last().unwrap().header.prev_hash;
                self.have_all_predecessors(block_prev_hash)
            } else {
                false
            }
        }
    }
}
