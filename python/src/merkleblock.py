#!/usr/bin/python3
import struct
import unittest
import functools
import math

from typing import List, Any, Dict
from io import BytesIO

from p2p_framework.object import CBlockHeader
from p2p_framework.serial import deser_uint256, ser_varint, ser_uint256_vector

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


def branches_to_flags(branches: List[Dict[str, str]]) -> bytes:
    """ Given the branches return the flags as bytes, the first byte contains the number of bytes
    """
    # Flag bits 0 = provided, 1 = calculated
    flags = [0b01 if b['pos'] == "L" else 0b10 for b in branches[::-1]]

    # Calculate number of bytes..
    byte_count = math.ceil((len(flags) * 2 + 1) / 8.0)
    print(f"flags = {flags}, byte_count = {byte_count}")
    ff = functools.reduce(lambda x, y: x << 2 | y, flags, 0b1)
    print(f"ff = {ff:b} , {ff:x}")
    retval = byte_count.to_bytes(1, "big") + ff.to_bytes(byte_count, "big")
    return retval


def create_merkleblock(blockheader: List[Any], hash: str, txs: List[str]) -> str:
    """ Given blockheader, tx hash, list of tx hashes return a merkleblock as a hexstr
    """
    print(f"create_merkleblock {blockheader}, {hash} {txs}")
    # Check that hash is in the list of transactions
    assert hash in txs

    # Create the blockheader
    bh: CBlockHeader = create_blockheader(blockheader)
    serialised_blockheader = bh.serialize()

    # Total number of tx
    number_of_txs = len(txs)
    print(f"len(number_of_txs)={number_of_txs}")

    # Create the merkle branch...
    if number_of_txs == 1:
        # Special case of one tx (tx_hash == merkle_root), therefore only need the blockheader
        # TODO check this for merkleblock
        return serialised_blockheader.hex()

    # Create tree
    tree = create_tree(txs)

    # Position of transaction of interest in the list
    pos = txs.index(hash)
    branches = walk_tree_from_pos(tree, pos, hash)

    # Number of hashes in branch
    print(branches)
    hashes = [hexstr_to_uint256(b['hash']) for b in branches]
    number_of_hashes: int = len(hashes)

    # Flag bits 0 = provided, 1 = calculated
    flag_bytes: bytes = branches_to_flags(branches)
    print(f"flag_bytes = {flag_bytes.hex()}")

    # Build merkleblock
    # The first 6 fields of the merkleblock are the blockheader
    # Followed by
    # * the number of transactions, LE
    # * number of hashes varint
    # * hashes 32 bytes * number of hashes
    # * flag bits - count + bytes...
    merkleblock: bytes = serialised_blockheader + \
        struct.pack("<I", number_of_txs) + \
        ser_varint(number_of_hashes) + \
        ser_uint256_vector(hashes) + \
        flag_bytes
    return merkleblock.hex()


class BlockTests(unittest.TestCase):
    def test_merkleblock(self):
        """
        Test using tx a2931fb3da9fb81c78f4183c550d29c52604a0dbbcf0ea8360805650020e9c7a
        """
        blockheader = [1507697, 536870912, '00000000000255ce95bab8646b63994698d65a4e4b70ab62b7d2d4c3d93c5414', 'ec1de7278677f73b4684575cb5d33e9ecddf43da5282795210774d59110ebb14', 1661156636, 436611740, 889702180]
        hash = "a2931fb3da9fb81c78f4183c550d29c52604a0dbbcf0ea8360805650020e9c7a"
        txs = ['863f79662b04207021d3a762663ceedc5c44a3d07caacb295a10bd4f2e7d0035', 'a2931fb3da9fb81c78f4183c550d29c52604a0dbbcf0ea8360805650020e9c7a', '8af40ec721024cc84ffb3a2a60a1b6a02579ba2679e0f25e4908072d090d085b', 'acb793e1e46a52a0c7f3f3229e17bf8be1567b5ded352d63cf6da5040f0bc5d7', '0ea5b9d6714622a79c97f65e7990a8e3a41caefcd5f538de1ade3e222c9d776e', 'ffc2754406195bea63b27f176309b5c620cf9fff2b812f51d2f8b986bbbd2ed5', 'd6730611705a64f3c31ed70c2e6be473edd1ea92283ce63362cae1ba29a48491', '698981570637cb8597dfd76506522339ff010a257dade3fcc04c7eeb0720115b', 'f663011f4fa6153719b3cbc3da86a9c24a2bc0434a32455c7774913a9e4810fd', '4fb316a25ded5d5b9415426d8585e20eff38c013e024dd16afc2cf60202657bd', '84b6264244911f957970d51d7821c37ba20cc6b5dc6cdda897086994e436ab9e', 'abfd310e1241229004c7d5280b34de806e720ba67568d25b13947ec2affa603e', 'fcb8697a22a189212862bed856195e86a80d161a39f6c30f4e81a3f008f7cd6a', '81f1d73742993ec92af13a30387679bdda52d5f142e87910d0f5408eb882468d', 'c44d9528bccc532e8907fffc5611ad3367aecb377744c452fff66979d3067725', '46a813cc0d9fcd6f1c80b77fa8426448c8b2db84a0c20fe162f67d2a38d6468d', 'fba344ad485229a62c1951d33b88493cdce30dacc51d479df213dda6e4e9be08', 'fd190e0d94ab17558977efdf443a058bfc28ed922ac493f43f486fce4d794a48', '0021f6a00aa9b5645d9729f52eea271b9be9e76dadd562c8e6a056dd6b6bdbfc', '86fb3c045e2007cc022af73408487b2344c9b3cdc576563d4f246c0909148c93', '3b75cc13f001b156143193309e47e4aedc2790ff4631c3af0f9f59862e6ca687', '0cf4dc61dbb47c6bee5af52a9b99c61d220f0ea7f3d455c01f394d6bab4c80c7', '4fa4420d21ae88ecd0b7dde3587976dbad9b805541eb2ac8dc0116ed53bb0861', 'e0e0568b1e86dc8a4e1cdb4591561930086730787ff6474e00ef8b9f6d9846db', '779644add34f9e68963bd775d60c80e283be8e9654533e5fe8acd978af610257', '525602f3dbf121c2bdceb5f453462d603cd65ccfa867888acbf9a878b25135b7', '3182408b9e39cdc5e86b592cea4ffa8383535e9c92f8969915e3c34a412199fb', '5fc27a10c5eda9f4e8f82622f27678bdc26710d09d47e02ad046a6d15df3d47a', 'a1ce5303a8780b57044b2b1f8d62bec73d679427b396e51c75448320850167c4', 'd7c1696558359b1c66eb6ae9a930c6a29bf41ac8288a38cc0c45102e92b4a7c6', 'c68ecd647a307d7e03a9c024cb2fc9a97dcdb257679d2c96fdd11734ede6b3b4', 'eb72e30f89fcc35221a7c26c56d90eb45f8a87306632b62865e7fc2a6cd01e7e', 'd8a84c1d88348f502ec19ca45e008adb0c6aebf036fc4a628c3492f6c34d2aeb', '7db46b8ff424cce4f0020fda46c1b1c0e0365492a54507b22b0f08b078a9eb52', 'cdbca6de2ca467364df3f3a51376f4974a1437a9ac1a2e1e19c29df668b6d389', '59e78a4786a654fd2799104b6bfd456610c86e13752dd3d77f19fe7353d13dae', 'f314629e3cadb1f0f90eaf5c5f3c433245a44ca84bfd2bbce428010d62d3e504', '644380ad5464ab5e7bd7687195dd1ef6bea24c178fa4e8c8acbb182b9b2f5425', 'cbaaa7b0fa42626d235525a13a81555e2e56d9da40b51e2aabab0e65da674a65', 'ea442f3008cabe98d8410ed08b2812cf5ec653edf2ecf7ab560007721d8369e0', '461e1c0a4a9012d1193e383123ad55d6ebb87ac2141619a5d72f73780363701a', '65e3da4abf45058ff32fc4d20a26e1023837226f615991626bc7326bfb06c8e8', 'ca7008923113ce740b2211054b026e57e3962b844963433b127e7ff087fc13b0', '77bd7e62961d4b39b58bef53f66cbc5870cd098a6b7bb157f045ddefeb23fd95', '86f530f5ba7c277f3641d3516b2b40496d5988ab40d4f3c31646293ef4a12209', '1cd1637d0c21f627f5d5ee1a2a59d603c802d59fb2ba62daab5f14aa52840d9a', 'cbe41b8920bf41095451ddff383990154000ad916b27160e8400cc38e7648048', '53695f647060df059adf74a018f753143b708400f3bd0c9171a0e0cefef7eaa1', '0c5b3ec2cfc65132ccc2a32c8317b24ef109cb024f41b2aacd8669cb159c3092', 'a63401276860f2338f7700adc224d51bc508e826f6e9311e6c7589d1382a6bca', '06f39d80e2af44735162151812860634b02580d72c1460550f2a05d873f27a5d', 'cb27d8c37d152f9445fb021c690b3097953fe1d9600935324e72760dacd49a4f', 'd3d53392caff65925ed984365df1442cc23d254fa70f2530802e4a79223eacf4', '6508dcee255221bd65d194254d8fca32acd43c11ff4eecfddf84e36d25bef162', '62c3ac79d8a6cf20b29dcf88f2d40bffe0c7f158a3f2c21a190222218227d5ba', '686d40e6bdbd81006c3f5ebcf956aa05f20f82341ce0bc3b557c99947750f753', '61eb20edc8d157e57c6e7653e6226120934043d94d77384adf8555392aa9d5fe', '21a5b9ba7e09ca6672ec17b48126726caddf9033172b865b6d0edee7fbae611b', '72447d5db587e4caec54336d0187da703b5f71196280e4ae0b4ef35a32871238', '0178c900a8e5b3a8a711059444affbca8aef45ff80d87d555c87766585f5578d', 'f8d9b7989b69ed9130d8272137bfbe2646b1210594ddabca22269a8ce3255102', 'ce741985c67ce7c62440d2940279bba85c5c2b98b370da3a79674401f695a560', '53ed183a08367dfb0bc4ac4683014ad74c862212542aa95e90145e7093b3bacb', '661791e4034000444ddff585aceae101d436433772c35c238ee0ebc5148a6674', 'a4cc912caab7b463bc2055e9efac469b28645b10c8f43d846930e5bd0dc5992c', 'bb4ab9a5c576705db71b43d5ce494816bfb0988a15e5b4a4b81def2880a87289', '038048c165557e6b643e570f61bee4bd5c429abc63a75bf6621512de44f6211b', 'bc0b79608047dcd1018d9d5de0de15cbf64222f8ad274ceee96dcfb6a0f19a32', 'd483daae398328cfefb45229b0dcc6e8cce03b25879114fb3f3583dd1ef21415', '9f3bfed5bbfbb5e4b5d1a1640563153008788a1be6735e83fc33371358d4512c', 'b329f79034d0a6634e0d9f55a02bd981d8e6cf09430bfb622fc5a73d3be13b14', '8bac5ba79c1586b76028f8e50384e8403502afb2a3f1b7fdea8c3dcfd843c793', 'eb6db7970457c93685a65a4753643af4cd515ff8a8e18bf79e2d8f6059da665e', '85e280684fbaee1e0cf104908db3d27856ae3f02f6fb8d6aab51a99ca44826e5', 'dbc9799067e5df92372e5dfecc4fe5af81f14477bad75572a4e517c7fa4c8592', 'fc9a5bc6f96f8592728fad232d0e70b45569d2bdd1a4855f7b5f4cc4e9f549d5', '4ae790994908ac15158c8452f1926869c3adcb6a33898ee72aec34480fba4644', 'd5b8df45e30cdc20a20d79fd08d3f33c5703c172a7940836cccc662d60a57ed7', '8a0968b3c29f01bbdebb44ec449114f1805509524b82fe01df0029cbbcb12a12', '3f1393c8a042517148e8a6b35f487e87541fbf5e1f9c7db8c7e492a6371faedf', 'e8e0f381b04ee9c9cb2d8b9dea22e2eb5d7c95ee742975207ae4e7ddebca57de']

        result = create_merkleblock(blockheader, hash, txs)
        self.assertEqual(result, "0000002000000000000255ce95bab8646b63994698d65a4e4b70ab62b7d2d4c3d93c5414ec1de7278677f73b4684575cb5d33e9ecddf43da5282795210774d59110ebb141c3d03639c2a061a24c70735510000000707863f79662b04207021d3a762663ceedc5c44a3d07caacb295a10bd4f2e7d00357cb0cd461ac4362103431acbbd12bdcffa57592dc2fe52b6667a5e1783540efabfa2df0495c05db4b041e16b0018c456f86582572adc300d5f2b29387c93496cb128e80e40eb5140bf657c75d0b871b2d2f036dc8955c99c2b993b6a8b7b0f6f08338ff57e3f44fdc58577dadf0d23ab90912ade55a1ff80e2e07ce1a53b7d27e76426d363bcff7f28c3718e9e07995c6e6fa666a6b144e4bb2f1dc797f1c0a826dc1c031ed64db6451aff5c37fc3a4af252fcb04f7aa4ff24b87d48e71a039b026aa9")


"""
def main():
    pass
    x = 0x03b55635.to_bytes(4, byteorder='little')
    print(x)
    n = bytes_to_bit_field(x)
    print(f"n={n}")
    show_bit_fields(n)
"""

if __name__ == '__main__':
    unittest.main()
