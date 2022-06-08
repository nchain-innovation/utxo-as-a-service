# Tools

The directory `python/src/tools` contains tools that have been used during project development.

* `fix_block_file.py` - This fixed an issue in which the blocks in the block file have an offset of 0 in the database
* `load_block.py` - This loaded the blocks from the blockfile and printed the number of blocks loaded
* `show_blocks.py` - This loaded the blocks from the blockfile and printed them out
* `truncate_blocks.py` - This loaded the blocks from the blockfile, truncated at a defined hash and wrote them out to a new file
* `fix_height_block_table.py` - This corrected the block table to have a height the same as the blockchain