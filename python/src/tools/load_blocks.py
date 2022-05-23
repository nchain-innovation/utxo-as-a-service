#!/usr/bin/python3
import sys

sys.path.append('..')
from io import BytesIO
from typing import List
from p2p_framework.object import CBlock


def load_blocks(fname: str) -> List[CBlock]:
    """ Load the blocks into List.
        Note tried loading blocks into a set to prevent duplicates and then convert the set to a list.
        However the set did not spot duplicates.
    """

    """ Load the blocks into a set to prevent duplicates and then convert the set to a list.
    """
    blocks: List[CBlock] = []
    try:
        with open(fname, "rb") as fh:
            f = BytesIO(fh.read())
            while True:
                block = CBlock()
                try:
                    block.deserialize(f)
                except:
                    break
                else:
                    blocks.append(block)
    except FileNotFoundError as e:
        print(f"load_blocks - File not found: {e}")

    return blocks


def main():
    """ Load block file in Python"""
    blockfile = "../../../data/block.dat"
    blocks = load_blocks(blockfile)
    print(len(blocks))


if __name__ == '__main__':
    main()
