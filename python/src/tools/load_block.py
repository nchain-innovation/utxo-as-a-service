#!/usr/bin/python3
import sys

sys.path.append('..')

from blockfile import load_block_at_offset


def main():
    """
    Quick test of loading a block at an offset
    """
    blockfile = "../../../data/block.dat"
    block = load_block_at_offset(blockfile, 54119162)
    block.rehash()
    print(block)
    print(block.to_dict())


if __name__ == '__main__':
    main()
