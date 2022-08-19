#!/usr/bin/python3
from typing import List, Any
from io import BytesIO

from p2p_framework.object import CBlockHeader
from p2p_framework.serial import deser_uint256

from merkle import create_tree, walk_tree_from_pos, MerkleTree, merkle_parent

""" This file creates a merkleblock which is a block header + proof of inclusion
"""


class MerkleTreeWithMerkleBlock(MerkleTree):
    """ MerkleTree with added MerkleBlock functionality
    """
    def populate_tree(self, flag_bits: List[int], hashes: List[bytes]):
        while self.root is None:
            if self.is_leaf():
                # leaf nodes have hash
                flag_bits.pop(0)
                self.set_current_node(hashes.pop(0))
                self.up()
            else:
                left_hash = self.get_left_node()
                if left_hash is None:
                    if flag_bits.pop(0) == 0:
                        self.set_current_node(hashes.pop(0))
                        self.up()
                    else:
                        self.left()

                elif self.right_exists():
                    right_hash = self.get_right_node()
                    if right_hash is None:
                        self.right()
                    else:
                        self.set_current_node(merkle_parent(left_hash, right_hash))
                        self.up()
                else:
                    self.set_current_node(merkle_parent(left_hash, right_hash))
                    self.up()

        if len(hashes) != 0:
            raise RuntimeError("Hashes not all consumed {}".format(len(hashes)))
        for flag_bit in flag_bits:
            if flag_bit != 0:
                raise RuntimeError("Not all flag bits comsumed")


def hexstr_to_uint256(hexstr: str) -> int:
    """ Convert hexstr to uint256
    """
    return deser_uint256(BytesIO(bytes.fromhex(hexstr)))


def create_blockheader(blockheader: List[Any]) -> CBlockHeader:
    (height, version, prev_hash, merkle_root, timestamp, bits, nonce) = tuple(blockheader)
    bh = CBlockHeader()
    bh.nVersion = version
    bh.hashPrevBlock = hexstr_to_uint256(prev_hash)
    bh.hashMerkleRoot = hexstr_to_uint256(merkle_root)
    bh.nTime = timestamp
    bh.nBits = bits
    bh.nNonce = nonce
    return bh


def bytes_to_bit_field(in_bytes: bytes) -> List[int]:
    flag_bytes = []
    for b in in_bytes:
        for _ in range(8):
            flag_bytes.append(b & 1)
            b >>= 1
    return flag_bytes


def show_bit_fields(bits: List[int]):
    print(f"len(n)={len(bits)}")
    n = 1  # number of entries on this row
    i = 0  # index in row
    print(" " * int((len(bits) / 2) - n), end='')
    for b in bits:
        print(f" {b}", end='')
        i += 1
        if i == n:
            print()
            i = 0
            n *= 2
            print(" " * int((len(bits) / 2) - n), end='')
        else:
            print(" ", end='')

    print()


def create_merkleblock(blockheader: List[Any], hash: str, txs: List[str]) -> str:
    """ Given blockheader, tx hash, list of tx hashes return a merkleblock as a hexstr
    """
    # Check that hash is in the list of transactions
    assert hash in txs
    bh = create_blockheader(blockheader)
    b = bh.serialize()
    # Create the merkle branch...
    if len(txs) == 1:
        # Special case of one tx (tx_hash == merkle_root), therefore only need the blockheader
        return b.hex()

    # Create tree
    tree = create_tree(txs)
    # position of transaction of interest in the list
    pos = txs.index(hash)
    branches = walk_tree_from_pos(tree, pos, hash)
    # total number of tx
    print(f"total no of tx = {len(txs)}")
    # number of hashes in branch
    print(f"len(branches)={len(branches)}")
    print(branches)
    # flag bits

    # Build merkleblock

    return b.hex()


def main():
    pass
    x = 0x03b55635.to_bytes(4, byteorder='little')
    print(x)
    n = bytes_to_bit_field(x)
    print(f"n={n}")
    show_bit_fields(n)


if __name__ == '__main__':
    main()
