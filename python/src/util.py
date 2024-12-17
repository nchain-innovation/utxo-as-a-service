import logging
from io import BytesIO
from typing import List, Dict

from p2p_framework.object import CBlock
from p2p_framework.hash import hash256


LOGGER = logging.getLogger(__name__)


# Blocks
def load_blocks(fname: str) -> List[CBlock]:
    """ Load the blocks into List.
        Note tried loading blocks into a set to prevent duplicates and then convert the set to a list.
        However the set did not spot duplicates.
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
        LOGGER.warning(f"load_config - File not found error {e}")

    # Ensure that all the block headers have their hash
    list(map(lambda b: b.calc_sha256(), blocks))
    # Remove duplicates
    y = {b.hash: b for b in blocks}
    retval = list(y.values())
    # Sort
    retval.sort(key=lambda x: x.nTime)
    return retval


def sort_blocks_by_hash_from_last(blocks: List[CBlock]) -> List[CBlock]:
    """ Given a list of  blocks sort by hash order
        Note that the input list must be:
        * time sorted
        * with hashes
    """
    new_blocks: List[CBlock] = []
    if len(blocks) > 0:
        # Create a mapping to make finding a block by hash quicker
        hash_to_index: Dict[str, int] = {b.hash: i for i, b in enumerate(blocks) if b.hash is not None}
        # Work from the last block
        b: CBlock = blocks[-1]
        new_blocks.append(b)
        while len(new_blocks) < len(blocks):
            # Will throw a key error here if can not find prev block
            b = blocks[hash_to_index[b.hashPrevBlock_as_hex_str()]]
            new_blocks.insert(0, b)
    return new_blocks


def sort_blocks_by_hash_from_first(blocks: List[CBlock]) -> List[CBlock]:
    """ Given a list of blocks sort by hash order
        Note that the input list must be:
        * time sorted
        * with hashes
    """
    new_blocks: List[CBlock] = []
    if len(blocks) > 0:
        # Create a mapping to make finding a block by hash quicker
        prev_hash_to_index: Dict[str, int] = {b.hashPrevBlock_as_hex_str(): i for i, b in enumerate(blocks) if b.hash is not None}
        # Work from the first block
        b: CBlock = blocks[0]
        new_blocks.append(b)
        while len(new_blocks) < len(blocks):
            # Will throw a key error here if can not find prev block
            assert b.hash is not None
            try:
                index = prev_hash_to_index[b.hash]
            except KeyError:
                # Last entry
                if b.hash == blocks[-1].hash:
                    # fine just append to the end
                    new_blocks.append(b)
                    break
                else:
                    # This should not occur
                    assert False
            else:
                try:
                    b = blocks[index]
                except KeyError:
                    # Can not find the next block by hash, how about returning the next block
                    index = prev_hash_to_index[b.hashPrevBlock_as_hex_str()] + 1
                    b = blocks[index]
                    while b in new_blocks:
                        index += 1
                        b = blocks[index]

            new_blocks.append(b)
    return new_blocks


def address_to_public_key_hash(addr: str) -> bytes:
    BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

    num: int = 0
    for c in addr:
        num *= 58
        num += BASE58_ALPHABET.index(c)
    combined: bytes = num.to_bytes(25, byteorder="big")
    checksum: bytes = combined[-4:]
    if hash256(combined[:-4])[:4] != checksum:
        raise ValueError(
            "bad address: {!r} {!r}".format(checksum, hash256(combined[:-4])[:4])
        )
    return combined[1:-4]


if __name__ == '__main__':
    import unittest

    class Addr2PKHTest(unittest.TestCase):

        def test_address_to_public_key_hash(self):
            address = "mgzhRq55hEYFgyCrtNxEsP1MdusZZ31hH5"
            calculated_public_key = address_to_public_key_hash(address).hex()
            public_key_hash = "10375cfe32b917cd24ca1038f824cd00f7391859"

            self.assertEqual(calculated_public_key, public_key_hash)

    unittest.main()
