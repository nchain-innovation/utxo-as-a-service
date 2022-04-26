use mysql::PooledConn;
use std::fs::OpenOptions;
use std::io::Read;

use crate::config::Config;
use sv::messages::Block;
use sv::util::Serializable;

pub struct BlockManager {
    start_block_hash: String,
    block_file: String,
    blocks: Vec<Block>,
}

impl BlockManager {
    pub fn new(config: &Config, _conn: PooledConn) -> Self {
        let mut b = BlockManager {
            start_block_hash: config.service.start_block_hash.clone(),
            block_file: config.shared.block_file.clone(),
            blocks: Vec::new(),
        };
        b.read_blocks();
        b
    }

    fn write_block(&self, block: &Block) {
        // Write a block to a block file
        let mut file = OpenOptions::new()
            .append(true)
            .open(&self.block_file)
            .unwrap();
        block.write(&mut file).unwrap();
    }

    fn read_blocks(&mut self) {
        // read blocks from a file
        let mut file = OpenOptions::new()
            .read(true)
            .open(&self.block_file)
            .unwrap();

        while let Ok(block) = Block::read(&mut file) {
            // dbg!(&block);
            self.blocks.push(block);
        }
        println!("{} blocks read", self.blocks.len());
    }

    pub fn add_block(&mut self, block: Block) {
        self.write_block(&block);
        self.blocks.push(block);
    }

    // pub fn on_block(&self, _block: Block) {}

    pub fn get_last_known_block_hash(&self) -> &str {
        self.start_block_hash.as_str()
    }
}
