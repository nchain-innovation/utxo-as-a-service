#!/usr/bin/python3
import time
from typing import List
import sys

sys.path.append('..')

from p2p_framework.object import CBlock
from util import load_blocks


def display_blocks(blocks: List[CBlock]) -> None:
    for b in blocks:
        print(f"{time.ctime(b.nTime)} - {b.hash}")
    print(f"len(blocks) = {len(blocks)}")


def check_for_gaps(blocks: List[CBlock]) -> None:
    last_good_hash = blocks[0].hash
    assert last_good_hash is not None
    for b in blocks[1:]:
        if b.hashPrevBlock_as_hex_str() != last_good_hash:
            print(f"gap found at {time.ctime(b.nTime)} - {b.hash} - Prev = {b.hashPrevBlock_as_hex_str()}")
            print(f" {b.hashPrevBlock_as_hex_str()}")
            print(f" {last_good_hash} - {b.hashPrevBlock_as_hex_str() != last_good_hash}")
            print()
        last_good_hash = b.hash


def main(fname: str) -> None:
    blocks = load_blocks(fname)
    blocks.sort(key=lambda x: x.nTime)

    # Sort by hash
    # new_blocks = sort_blocks_by_hash_from_last(blocks)
    # new_blocks = sort_blocks_by_hash_from_first(blocks)

    # Display blocks
    # display_blocks(blocks)

    # Check for gaps
    check_for_gaps(blocks)


if __name__ == '__main__':
    # config: MutableMapping[str, Any] = load_config("../data/uaas.toml")
    # blockfile = "../" + config["Blocks"]["block_file"]
    blockfile = "../../../data/block.dat"
    main(blockfile)
