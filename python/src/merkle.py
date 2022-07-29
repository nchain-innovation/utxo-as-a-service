import math
import time
import unittest
from typing import List, Dict, Union
from p2p_framework.hash import hash256

NodeType = Union[None, bytes]
NodeListType = List[NodeType]


def node_to_str(value: NodeType) -> str:
    """ Given a node return its value as a str
    """
    if isinstance(value, bytes):
        return value[::-1].hex()
    else:
        return "None"


def str_to_node(s: str) -> NodeType:
    assert isinstance(s, str)
    return bytes.fromhex(s)[::-1]


class MerkleTree:
    """ Used to represent merkle trees
    """
    def __init__(self, total: int):
        self.total: int = total
        self.max_depth: int = math.ceil(math.log(self.total, 2))
        self.nodes: List[NodeListType] = []

        for depth in range(self.max_depth + 1):
            num_items = math.ceil(self.total / 2 ** (self.max_depth - depth))
            level_hashes: NodeListType
            level_hashes = [None] * num_items
            self.nodes.append(level_hashes)
        # Pointer to location in tree
        self.current_depth: int = 0
        self.current_index: int = 0

    def __repr__(self):
        result: List[str] = []
        level: NodeListType
        depth: int
        for depth, level in enumerate(self.nodes):
            items: List[str] = []
            h: NodeType
            for index, h in enumerate(level):
                if h is None:
                    short = 'None'
                else:
                    short = "{}...".format(h.hex()[:8])
                if depth == self.current_depth and index == self.current_index:
                    items.append("*{}*".format(short[:-2]))
                else:
                    items.append("{}".format(short))
            result.append(", ".join(items))
        return "\n".join(result)

    def up(self):
        self.current_depth -= 1
        self.current_index //= 2

    def left(self):
        self.current_depth += 1
        self.current_index *= 2

    def right(self):
        self.current_depth += 1
        self.current_index = self.current_index * 2 + 1

    def root(self) -> NodeType:
        return self.nodes[0][0]

    def set_current_node(self, value: bytes):
        self.nodes[self.current_depth][self.current_index] = value

    def get_current_node(self) -> NodeType:
        return self.nodes[self.current_depth][self.current_index]

    def get_left_node(self) -> NodeType:
        return self.nodes[self.current_depth + 1][self.current_index * 2]

    def get_right_node(self) -> NodeType:
        return self.nodes[self.current_depth + 1][self.current_index * 2 + 1]

    def is_leaf(self) -> bool:
        return self.current_depth == self.max_depth

    def at_root(self) -> bool:
        return self.current_depth == 0 and self.current_index == 0

    def right_exists(self) -> bool:
        return len(self.nodes[self.current_depth + 1]) > self.current_index * 2 + 1

    def calc_root(self):
        while self.root() is None:
            if self.is_leaf():
                self.up()
            else:
                left_hash = self.get_left_node()
                if left_hash is None:
                    self.left()
                elif self.right_exists():
                    right_hash = self.get_right_node()
                    if right_hash is None:
                        self.right()
                    else:
                        self.set_current_node(hash256(left_hash + right_hash))
                        self.up()
                else:
                    self.set_current_node(hash256(left_hash + left_hash))
                    self.up()

    def get_merkle_root(self) -> str:
        # return the merkle root as a string
        return node_to_str(self.root())


def create_tree(hex_hashes: List[str]) -> MerkleTree:
    tree = MerkleTree(len(hex_hashes))
    # Set tree bottom row of transaction hashes
    tree.nodes[tree.max_depth] = [str_to_node(h) for h in hex_hashes]

    tree.calc_root()
    return tree


def walk_tree_from_pos(tree: MerkleTree, pos: int, hash: str) -> List[Dict[str, str]]:
    # this assumes that the tree has been fully built
    branches: List[Dict[str, str]] = []
    # Work up from the bottom
    tree.current_depth = tree.max_depth
    tree.current_index = pos
    start_node = str_to_node(hash)
    while True:
        tree.up()
        left_hash = tree.get_left_node()
        if tree.right_exists():
            if left_hash == start_node:
                value = tree.get_right_node()
                branches.append({"hash": node_to_str(value), "pos": "R"})
            else:
                value = tree.get_left_node()
                branches.append({"hash": node_to_str(value), "pos": "L"})
        else:
            value = tree.get_left_node()
            branches.append({"hash": node_to_str(value), "pos": "L"})

        start_node = tree.get_current_node()

        # Quit when we get to the top of the tree
        if tree.at_root():
            break
    return branches


def create_merkle_branch(hash: str, txs: List[str]) -> List[Dict[str, str]]:
    # Check that hash is in the list of transactions
    assert hash in txs
    if len(txs) == 1:
        # special case of one tx (tx_hash == merkle_root)
        return []
    start = time.time()
    tree = create_tree(txs)
    elapsed_time = time.time() - start
    print(f"time to create tree {elapsed_time}")
    # position of transaction of interest in the list
    pos = txs.index(hash)
    branches = walk_tree_from_pos(tree, pos, hash)
    return branches


class BranchTests(unittest.TestCase):
    def test_2_1st(self):
        tx = "931475bee79c76509ccb01916998009c93afd54f4bcce431299848b473d53aef"
        txs = [
            "931475bee79c76509ccb01916998009c93afd54f4bcce431299848b473d53aef",
            "d3dc8224dc896986cebb8bf78cb658c8ca7b85c1b99077d835dee5f81424e7b9"
        ]
        branch = create_merkle_branch(tx, txs)
        expected_branch = [{"hash": "d3dc8224dc896986cebb8bf78cb658c8ca7b85c1b99077d835dee5f81424e7b9", "pos": "R"}]
        self.assertEqual(branch, expected_branch)

    def test_2_2nd(self):
        tx = "d3dc8224dc896986cebb8bf78cb658c8ca7b85c1b99077d835dee5f81424e7b9"
        txs = [
            "931475bee79c76509ccb01916998009c93afd54f4bcce431299848b473d53aef",
            "d3dc8224dc896986cebb8bf78cb658c8ca7b85c1b99077d835dee5f81424e7b9"
        ]
        branch = create_merkle_branch(tx, txs)
        expected_branch = [{"hash": "931475bee79c76509ccb01916998009c93afd54f4bcce431299848b473d53aef", "pos": "L"}]
        self.assertEqual(branch, expected_branch)

    def test_3_1st(self):
        tx = "779d313658cd99d9adb8446521552301e5bb29eb74eff84d562c9954885d6747"
        txs = [
            "779d313658cd99d9adb8446521552301e5bb29eb74eff84d562c9954885d6747",
            "791161487f7411a3a937f287cab52b22ff4ca223c2409af4b99587c8b694560d",
            "f3ff50949f82b6eaa0fc8dc0d4764687117128c3cee73025b13ed1bd88f4e1d7"]
        branch = create_merkle_branch(tx, txs)
        expected_branch = [
            {"hash": "791161487f7411a3a937f287cab52b22ff4ca223c2409af4b99587c8b694560d", "pos": "R"},
            {"hash": "553e149e13bf81daa893e2517d5f438f1ccb166c405e214ca7a1d2fc31c284dd", "pos": "R"}]
        self.assertEqual(branch, expected_branch)

    def test_3_2nd(self):
        tx = "791161487f7411a3a937f287cab52b22ff4ca223c2409af4b99587c8b694560d"
        txs = [
            "779d313658cd99d9adb8446521552301e5bb29eb74eff84d562c9954885d6747",
            "791161487f7411a3a937f287cab52b22ff4ca223c2409af4b99587c8b694560d",
            "f3ff50949f82b6eaa0fc8dc0d4764687117128c3cee73025b13ed1bd88f4e1d7"]
        branch = create_merkle_branch(tx, txs)
        expected_branch = [
            {"hash": "779d313658cd99d9adb8446521552301e5bb29eb74eff84d562c9954885d6747", "pos": "L"},
            {"hash": "553e149e13bf81daa893e2517d5f438f1ccb166c405e214ca7a1d2fc31c284dd", "pos": "R"}]
        self.assertEqual(branch, expected_branch)

    def test_3_3rd(self):
        tx = "f3ff50949f82b6eaa0fc8dc0d4764687117128c3cee73025b13ed1bd88f4e1d7"
        txs = [
            "779d313658cd99d9adb8446521552301e5bb29eb74eff84d562c9954885d6747",
            "791161487f7411a3a937f287cab52b22ff4ca223c2409af4b99587c8b694560d",
            "f3ff50949f82b6eaa0fc8dc0d4764687117128c3cee73025b13ed1bd88f4e1d7"]
        branch = create_merkle_branch(tx, txs)
        expected_branch = [
            {"hash": "f3ff50949f82b6eaa0fc8dc0d4764687117128c3cee73025b13ed1bd88f4e1d7", "pos": "L"},
            {"hash": "7109e178183e6198b293ed261e370ba3f353038e557baac317f017b5420ff2c5", "pos": "L"}]
        self.assertEqual(branch, expected_branch)

    def test_5_1st(self):
        tx = "eafcf1a1e2c8694433fdec50fdb0020c89decd2049d5bfad2cb0d5f61f00a049"
        txs = [
            "eafcf1a1e2c8694433fdec50fdb0020c89decd2049d5bfad2cb0d5f61f00a049",
            "48a1542f7f2b8385a049a28635ab4276ebcaf424dd4b37506e9af816de20f234",
            "507a38f1f380107f4e5039f0204b49cf23f676f80ef9a97cf5fdaa275099db45",
            "bead66e14d905be19b64c4b26dd19d5db6127ffb7a5f019ce1856f97278e8eae",
            "c16407dc758f6d35d1502cecf0546d17e17679c12e9500217986e5c32f56e694"]
        branch = create_merkle_branch(tx, txs)
        expected_branch = [
            {"hash": "48a1542f7f2b8385a049a28635ab4276ebcaf424dd4b37506e9af816de20f234", "pos": "R"},
            {"hash": "4fe7355a0a96b6f1a415bc8d91f5f6a586701692f4e4e8c96f712d70324aa275", "pos": "R"},
            {"hash": "074e67ffa1feb37c979aac90f43174a7bac0399c833da0d9bf257072b9186bc0", "pos": "R"}]
        self.assertEqual(branch, expected_branch)

    def test_5_5th(self):
        tx = "c16407dc758f6d35d1502cecf0546d17e17679c12e9500217986e5c32f56e694"
        txs = [
            "eafcf1a1e2c8694433fdec50fdb0020c89decd2049d5bfad2cb0d5f61f00a049",
            "48a1542f7f2b8385a049a28635ab4276ebcaf424dd4b37506e9af816de20f234",
            "507a38f1f380107f4e5039f0204b49cf23f676f80ef9a97cf5fdaa275099db45",
            "bead66e14d905be19b64c4b26dd19d5db6127ffb7a5f019ce1856f97278e8eae",
            "c16407dc758f6d35d1502cecf0546d17e17679c12e9500217986e5c32f56e694"]
        branch = create_merkle_branch(tx, txs)
        expected_branch = [
            {"hash": "c16407dc758f6d35d1502cecf0546d17e17679c12e9500217986e5c32f56e694", "pos": "L"},
            {"hash": "76bcb15456f4efbcae13a5c3e0205358d92f50d4c98f4a7c574bcb11bbcd24b1", "pos": "L"},
            {"hash": "40a129b27ab77c3dff72dc3e459c3f30293dafd54b5a0ae93779433265368422", "pos": "L"}]
        self.assertEqual(branch, expected_branch)


""" Merkle notes
curl --location --request GET  "https://api.whatsonchain.com/v1/bsv/test/tx/c16407dc758f6d35d1502cecf0546d17e17679c12e9500217986e5c32f56e694/proof"

"""

if __name__ == '__main__':
    unittest.main()
