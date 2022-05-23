#!/usr/bin/python3
from typing import List, Dict
from io import BytesIO

import sys
sys.path.append('..')

from blockfile import load_block_at_offset
from database import database
from util import load_config
from p2p_framework.object import CBlock


def quick_test(offset):
    """
    Quick test of loading a block at an offset
    """
    blockfile = "../../data/block.dat"
    block = load_block_at_offset(blockfile, offset)
    block.rehash()
    print(block)
    print(block.to_dict())


def find_0_offset_blocks() -> List[str]:
    retval = database.query("SELECT hash FROM blocks WHERE offset = 0;")
    retval = list(map(lambda x: x[0], retval))
    return retval


def load_blockhash_and_offset(fname: str) -> Dict[str, int]:
    """ Given a filename determine the hash and offset of all blocks contained in it
    """
    hash_to_offset: Dict[str, int] = {}
    try:
        with open(fname, "rb") as fh:
            f = BytesIO(fh.read())
            while True:
                block = CBlock()
                try:
                    pos = f.tell()
                    block.deserialize(f)
                except:
                    break
                else:
                    block.calc_sha256()
                    hash = block.hash
                    if hash is not None:
                        hash_to_offset[hash] = pos
    except FileNotFoundError as e:
        print(f"load_blocks - File not found: {e}")

    return hash_to_offset


def main():
    """ This fixed an issue in which the blocks in the block file have an offset of 0
    """
    blockfile = "../../data/block.dat"

    config = load_config("../../data/uaasr.toml")
    database.set_config(config)

    # First find the 0 offset blocks in database
    zero_offset_blocks = find_0_offset_blocks()

    # Given a hash find the offset in the file
    hash_to_offset = load_blockhash_and_offset(blockfile)

    for hash in zero_offset_blocks:
        offset = hash_to_offset[hash]
        query = f"UPDATE blocks SET offset={offset} WHERE hash='{hash}';"
        # query = f"replace blocks where hash = '{hash}' (offset) VALUES ({offset});"
        print(query)
        r = database.query(query)
        print(r)


if __name__ == '__main__':
    main()
