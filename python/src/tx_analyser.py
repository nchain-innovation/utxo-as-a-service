import datetime
import time
from typing import List, Dict, Any, Optional

from database import database
from blockfile import blockfile
from merkle import create_merkle_branch
from p2p_framework.object import CTransaction
from config import ConfigType


class TxAnalyser:
    def __init__(self):
        self.complete = 6

    def set_config(self, config: ConfigType):
        self.complete = config['utxo']['complete']

    def _read_mempool(self) -> List[Dict[str, Any]]:
        # Read mempool from databaseß
        result = database.query("SELECT * FROM mempool")
        retval = [{
            "hash": f"{x[0]}", "locktime": x[1], "fee": x[2],
            "time": datetime.datetime.fromtimestamp(x[3]).strftime('%Y-%m-%d %H:%M:%S')
        } for x in result]

        return retval

    def get_mempool(self) -> Dict[str, List[Dict[str, Any]]]:
        """ Return a dictionary of mempool"""
        return {
            "mempool": self._read_mempool(),
        }

    def _read_utxo(self, hash: str) -> List[Dict[str, Any]]:
        # Read utxo from database
        result = database.query(f"SELECT * FROM utxo WHERE hash = '{hash}';")
        retval = [{
            "hash": f"{x[0]}", "pos": x[1], "satoshi": x[2],
            "height": x[3]
        } for x in result]
        return retval

    def get_utxo_entry(self, hash: str) -> Dict[str, List[Dict[str, Any]]]:
        """ Return the utxo entry identified by hash"""
        return {
            "utxo": self._read_utxo(hash),
        }

    def get_utxo_by_outpoint(self, hash: str, pos: int) -> Dict[str, Any]:
        # Read utxo from database
        result = database.query(f"SELECT * FROM utxo WHERE hash = '{hash}' AND pos = {pos};")
        return {"result": len(result) > 0}

    def get_utxo(self, pubkeyhash: str) -> Dict[str, Any]:
        # Return the UTXO associated with a particular pubkeyhash
        start = time.time()

        result = database.query(f"SELECT hash, pos, satoshis, height FROM utxo WHERE pubkeyhash = '{pubkeyhash}';")
        elapsed_time = time.time() - start
        print(f"Time to query {elapsed_time}")

        start = time.time()

        retval = [{
            "hash": f"{x[0]}", "pos": x[1], "satoshi": x[2],
            "height": x[3]
        } for x in result]

        elapsed_time = time.time() - start
        print(f"Time to process {elapsed_time}")

        print(f"retval.len() = {len(retval)}")
        return {
            "utxo": retval,
        }

    def get_balance(self, pubkeyhash: str, blockheight: int) -> Dict[str, Any]:
        # Return the UTXO balance with a particular pubkeyhash
        result = database.query(f"SELECT satoshis, height FROM utxo WHERE pubkeyhash = '{pubkeyhash}';")
        confirmed_height = blockheight - self.complete

        confirmed = sum([x[0] for x in result if x[1] <= confirmed_height])
        unconfirmed = sum([x[0] for x in result if x[1] > confirmed_height])
        return {
            "confirmed": confirmed,
            "unconfirmed": unconfirmed,
        }

    def _read_block_offset(self, hash: str) -> Optional[int]:
        # Read block offset based on tx hash from database
        result = database.query(
            f"SELECT offset FROM blocks INNER JOIN tx on tx.height = blocks.height where tx.hash='{hash}';")
        try:
            return result[0][0]
        except IndexError:
            return None

    def _read_tx_height_and_blockindex(self, hash: str) -> Optional[List[int]]:
        result = database.query(
            f"SELECT height, blockindex FROM tx WHERE hash='{hash}';")
        try:
            return result[0]
        except IndexError:
            return None

    def decode_tx(self, hash: str, tx: CTransaction) -> Dict[str, Any]:
        """ Given a transaction decode it to a dictionary
        """
        tx_as_dict = tx.to_dict()

        # Get utxo
        utxo_entry = self._read_utxo(hash)
        # Create a list of unspent pos
        utxo = list(map(lambda x: x['pos'], utxo_entry))
        for pos, vout in enumerate(tx_as_dict['vout']):
            vout["spent"] = pos not in utxo

        height_and_blockindex = self._read_tx_height_and_blockindex(hash)
        if height_and_blockindex is None:
            return {
                "hash": hash,
                "tx": tx_as_dict,
            }
        else:
            return {
                "hash": hash,
                "tx": tx_as_dict,
                "height": height_and_blockindex[0],
                "pos": height_and_blockindex[1],
            }

    def get_tx_entry(self, hash: str) -> Dict[str, Any]:
        """ Return the transaction entry identified by hash

            Return the tx as a dictionary
            Indicate if the tx outpoints have been spent or not

        """
        # Get tx
        offset = self._read_block_offset(hash)
        if offset is None:
            return {
                "tx": f"Transaction {hash} not found in block"
            }
        block = blockfile.load_at_offset(offset)
        tx = list(filter(lambda x: x.hash == hash, block.vtx))[0]
        return self.decode_tx(hash, tx)

    def get_tx_raw_entry(self, hash: str) -> Dict[str, Any]:
        """ return the serialised form of the transaction """
        offset = self._read_block_offset(hash)
        if offset is None:
            return {
                "tx": f"Transaction {hash} not found in block"
            }
        block = blockfile.load_at_offset(offset)
        tx = list(filter(lambda x: x.hash == hash, block.vtx))[0]
        b = tx.serialize()
        return {
            "tx": b.hex(),
        }

    def tx_exist(self, hash: str) -> bool:
        # Return true if txid is in txs or mempool or collection
        # self.txs.contains_key(&hash) || self.mempool.contains_key(&hash)
        txs = database.query(f"SELECT * FROM tx WHERE hash = '{hash}';")
        if len(txs) > 0:
            return True
        mempool = database.query(f"SELECT * FROM mempool WHERE hash = '{hash}';")
        if len(mempool) > 0:
            return True
        collection = database.query(f"SELECT * FROM collection WHERE hash = '{hash}';")
        if len(collection) > 0:
            return True
        return False

    def get_tx_merkle_proof(self, hash: str) -> Dict[str, Any]:
        # Given the txid return the merkle branch proof for a confirmed transaction
        # Get the block
        block = database.query(f"SELECT blocks.height, blocks.hash, merkle_root FROM blocks INNER JOIN tx on tx.height = blocks.height WHERE tx.hash='{hash}';")
        try:
            height = block[0][0]
            block_hash = block[0][1]
            merkle_root = block[0][2]
        except IndexError:
            return {
                "status": f"Transaction {hash} not found in block"
            }
        # Get the txs in the block
        result = database.query(f"SELECT hash FROM tx WHERE height = '{height}' ORDER BY blockindex ASC;")
        txs = [x[0] for x in result]
        # Create merkle proof
        branches = create_merkle_branch(hash, txs)
        return {
            "block_hash": block_hash,
            "merkle_root": merkle_root,
            "tx_hash": hash,
            "branches": branches,
        }


"""
curl --location --request GET  "https://api.whatsonchain.com/v1/bsv/main/tx/c1d32f28baa27a376ba977f6a8de6ce0a87041157cef0274b20bfda2b0d8df96/proof"
[{
    "blockHash":"0000000000000000091216c46973d82db057a6f9911352892b7769ed517681c3",
    "branches":[
        {"hash":"7e0ba1980522125f1f40d19a249ab3ae036001b991776813d25aebe08e8b8a50","pos":"R"},
        {"hash":"1e3a5a8946e0caf07006f6c4f76773d7e474d4f240a276844f866bd09820adb3","pos":"R"}
    ],
    "hash":"c1d32f28baa27a376ba977f6a8de6ce0a87041157cef0274b20bfda2b0d8df96",
    "merkleRoot":"95a920b1002bed05379a0d2650bb13eb216138f28ee80172f4cf21048528dc60"
}]

curl --location --request GET  "https://api.whatsonchain.com/v1/bsv/test/tx/a7362153a911704f247ddf0e82b370e6f982ea8e2b5adefc0396aaace19bdf87/proof"
[{
    "blockHash":"00000000000006221044332c2f7eaa928a3be87205dad6af48540b6deebf139d",
    "branches":[
        {"hash":"eb29e2c835cb768a28686b1197c7676725947304527584784be0b9d3c39984e5","pos":"R"},
        {"hash":"05cbefbeda1186faafc29846aa748f713e37cf6cd3afbc27d0b7bd944a992f67","pos":"L"},
        {"hash":"372e51252988c584a3fa4d17ceaf64b6482a65f3a31d2059a5ef4a33e4d798cf","pos":"L"},
        {"hash":"816467446ab29cb4023ab2848797f7052d9adb297239d1a624aa98d1f68f8b07","pos":"L"},
        {"hash":"5e40e668f18c0819c803010eee8249daa089863b334b138a7f790b092a231682","pos":"L"},
        {"hash":"20e409c75a215c8daa65a37e726ff146288035a7a0d611ea4c49f840355c14cc","pos":"L"},
        {"hash":"a191a73cf116300852f5934d42474f7f52c562dfe53d6821e7736326ca9ddbf1","pos":"R"}
    ],
    "hash":"a7362153a911704f247ddf0e82b370e6f982ea8e2b5adefc0396aaace19bdf87",
    "merkleRoot":"91caa91ecef2a2f42eee00995ef15135414d319b7ece67bbfe1fa499b155cb64"
}]

"""

tx_analyser = TxAnalyser()
