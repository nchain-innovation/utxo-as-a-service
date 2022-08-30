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


def save_block(fname: str, block: CBlock) -> None:
    with open(fname, "ab") as f:
        f.write(block.serialize())


def save_blocks(fname: str, blocks: List[CBlock], mode="wb+") -> None:
    with open(fname, mode) as f:
        for block in blocks:
            f.write(block.serialize())


def truncate_blocks(blocks: List[CBlock], target_hash: str) -> List[CBlock]:
    """ Stop at the provided hash"""
    retval = []
    for b in blocks:
        if b.hash != target_hash:
            retval.append(b)
        else:
            break
    return retval


def main():
    """ Load block file in Python"""
    blockfile = "../../../data/block.dat"
    blocks = load_blocks(blockfile)

    # Ensure that all the block headers have their hash
    list(map(lambda b: b.calc_sha256(), blocks))

    blocks = truncate_blocks(blocks, "000000000000a431fd46e70fbfef94773af4d3d0f54d93885b8c58443d872585")
    print(len(blocks))
    target = "../../../data/block2.dat"
    save_blocks(target, blocks)


if __name__ == '__main__':
    main()
