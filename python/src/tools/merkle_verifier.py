#!/usr/bin/python3
import unittest
import json
import argparse
import requests

import sys
sys.path.append('..')

from p2p_framework.hash import hash256
from util import load_config, ConfigType
from merkle import node_to_str, str_to_node


def verify_proof(proof: str) -> bool:
    r = json.loads(proof)

    tx_hash = r["tx_hash"]
    merkle_root = r["merkle_root"]
    branches = r["branches"]
    print(f"tx_hash={tx_hash}\n")

    working_hash = str_to_node(tx_hash)
    assert isinstance(working_hash, bytes)

    # work with hash up to merkle tree
    for i, branch in enumerate(branches):
        value = str_to_node(branch["hash"])
        assert isinstance(value, bytes)
        pos = branch["pos"]
        if pos == "R":
            working_hash = hash256(working_hash + value)
        else:
            working_hash = hash256(value + working_hash)
        print(f"* {i} working_hash={node_to_str(working_hash)}")
    print()
    calculated_merkle_root = node_to_str(working_hash)

    print(f"calculated_merkle_root={calculated_merkle_root}")
    print(f"merkle_root={merkle_root}")
    print()
    # compare result with merkle root
    return calculated_merkle_root == merkle_root


class VerificationTests(unittest.TestCase):
    def test_2_1st(self):
        PROOF = '{"block_hash":"00000000000004e157046be273f3e0d05ce257ca059e97245c2bc07c24c1bf4f","merkle_root":"847e947a615689a7b4c0bf09ad85c20c6206d1eab25d11d78cde0a298525674f","tx_hash":"ff4d80f428d244bbd313d14f6b3502e740f3e1dc4768b3115b8cf3734e586856","branches":[{"hash":"0bd3573b03203754d77f7d6c47a35e81d53bf149f015ad43ceae1e7a14b2a1d2","pos":"L"},{"hash":"e32b529f9573bd30443ee499c96756fa605a3041845e4628e0aab49833598e9e","pos":"L"},{"hash":"1efc9dc86232dc76ab61cd6bda4e9507db5da9ef1aaba760e15b108b66a0f093","pos":"R"},{"hash":"a9ed5525642b47cbe3abe4242da3f0fdbc60b86e2ead1be4b22002f4207f9a62","pos":"R"},{"hash":"487859b95cccaddc17172c0c02ebdb234ff25e9e3c6ba2f7dd8036d0eb9dcdad","pos":"R"},{"hash":"c86e37aaa832608ec063e740edea0c6d9507057d00ec904be4a34c2f1fe932dc","pos":"R"},{"hash":"3d40e87a98db919283965773b132105cc33ca3123ed2cc562c2de98c6f3385a7","pos":"R"},{"hash":"c0d43f577fde226685f3a208889e7039d1cf786320218f3d565cb468ccdde511","pos":"R"}]}'
        result = verify_proof(PROOF)
        self.assertTrue(result)


"""
curl -X 'GET' \
  'http://127.0.0.1:5010/tx/proof?hash=ff4d80f428d244bbd313d14f6b3502e740f3e1dc4768b3115b8cf3734e586856' \
  -H 'accept: application/json'
{"block_hash":"00000000000004e157046be273f3e0d05ce257ca059e97245c2bc07c24c1bf4f","merkle_root":"847e947a615689a7b4c0bf09ad85c20c6206d1eab25d11d78cde0a298525674f","tx_hash":"ff4d80f428d244bbd313d14f6b3502e740f3e1dc4768b3115b8cf3734e586856","branches":[{"hash":"0bd3573b03203754d77f7d6c47a35e81d53bf149f015ad43ceae1e7a14b2a1d2","pos":"L"},{"hash":"e32b529f9573bd30443ee499c96756fa605a3041845e4628e0aab49833598e9e","pos":"L"},{"hash":"1efc9dc86232dc76ab61cd6bda4e9507db5da9ef1aaba760e15b108b66a0f093","pos":"R"},{"hash":"a9ed5525642b47cbe3abe4242da3f0fdbc60b86e2ead1be4b22002f4207f9a62","pos":"R"},{"hash":"487859b95cccaddc17172c0c02ebdb234ff25e9e3c6ba2f7dd8036d0eb9dcdad","pos":"R"},{"hash":"c86e37aaa832608ec063e740edea0c6d9507057d00ec904be4a34c2f1fe932dc","pos":"R"},{"hash":"3d40e87a98db919283965773b132105cc33ca3123ed2cc562c2de98c6f3385a7","pos":"R"},{"hash":"c0d43f577fde226685f3a208889e7039d1cf786320218f3d565cb468ccdde511","pos":"R"}]}
"""


def request_merkle_proof(hash: str, python_url: str) -> str:
    result = requests.get(python_url + "/tx/proof", params={"hash": hash})
    return result.text


def request_merkleblock(hash: str, python_url: str) -> str:
    result = requests.get(python_url + "/tx/merkleblock", params={"hash": hash})
    return result.text


def process_args(args: argparse.Namespace, python_url: str) -> bool:
    if args.txid:
        proof = request_merkle_proof(args.txid, python_url)
        print(f"proof={proof}\n")

        if verify_proof(proof):
            print("Calculated merkle root and block merkle root match for this branch.")
            print("Therefore this transaction is contained in this block.")
        else:
            print("Calculated merkle root and block merkle root do not match for this branch.")
        return True
    elif args.block:
        proof = request_merkleblock(args.block, python_url)
        print(f"proof={proof}\n")

        return True

    else:
        return False


def main():
    print("merkle_verifier")
    print("---------------")
    config: ConfigType = load_config("../../data/uaasr.toml")
    python_url = "http://" + config["web_interface"]["address"]

    parser = argparse.ArgumentParser(description="Verify a transaction using a merkle tree.")
    parser.add_argument("-txid", metavar="HASH", help="the hash of the transaction that you wish to verify, in json format")
    parser.add_argument("-block", metavar="HASH", help="the hash of the transaction that you wish to verify, in merkleblock format")

    args = parser.parse_args()
    if not process_args(args, python_url):
        parser.print_help()


if __name__ == '__main__':
    main()
