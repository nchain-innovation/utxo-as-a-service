from typing import List, Dict, Any, Optional
import time
from database import database
from p2p_framework.object import CBlockHeader
import datetime
from mysql.connector.errors import ProgrammingError


class BlockManager:
    def __init__(self):
        self.start_block_height: int

    def _a_result_to_block(self, x) -> Dict[str, Any]:
        block = {
            "height": x[0],
            "header": {
                "hash": x[1],
                "version": f'{x[2]:08x}',
                "hashPrevBlock": x[3],
                "hashMerkleRoot": x[4],
                "nTime": time.ctime(x[5]),
                "nBits": f'{x[6]:08x}',
                "nNonce": f'{x[7]:08x}',
            },
            "blocksize": x[9],
            "number of tx": x[10],
        }
        return block

    def _a_result_to_hex_blockheader(self, x) -> Dict[str, Any]:
        json_notification = {
            "version": x[2],
            "hashPrevBlock": x[3],
            "hashMerkleRoot": x[4],
            "time": x[5],
            "bits": x[6],
            "nonce": x[7],
        }
        blockheader = CBlockHeader(header=None, json_notification=json_notification)
        b = blockheader.serialize()
        return {"block": b.hex()}

    def _results_to_hex_blockheader(self, results) -> Optional[Dict[str, Any]]:
        if results != []:
            x = results[0]
            return self._a_result_to_hex_blockheader(x)
        else:
            return None

    def _read_latest_blocks(self) -> List[Dict[str, Any]]:
        # Read blocks from database
        result = database.query("SELECT * FROM blocks ORDER BY height desc LIMIT 20;")
        retval = []
        for x in result:
            y = list(x)
            block = self._a_result_to_block(y)
            retval.append(block)
        return retval

    def get_latest_blocks(self) -> Dict[str, List[Dict[str, Any]]]:
        """ Return a dictionary of blocks"""
        return {
            "blocks": self._read_latest_blocks(),
        }

    def _results_to_block(self, results) -> Optional[Dict[str, Any]]:
        if results != []:
            x = results[0]
            return self._a_result_to_block(x)
        else:
            return None

    def _read_block_from_hash(self, hash) -> Optional[Dict[str, Any]]:
        """
        This reads a block without accessing the blockfile.
        As the blockfile can be large and expensive to read.
        """
        retval = database.query(f"SELECT * FROM blocks WHERE hash = '{hash}';")
        return self._results_to_block(retval)

    def _read_block_from_height(self, height) -> Optional[Dict[str, Any]]:
        """
        This reads a block without accessing the blockfile.
        As the blockfile can be large and expensive to read.
        """
        retval = database.query(f"SELECT * FROM blocks WHERE height = '{height}';")
        return self._results_to_block(retval)

    def _read_tx_at_height(self, height) -> List[str]:
        try:
            result = database.query(f"SELECT hash FROM tx WHERE height = '{height}' ORDER BY blockindex ASC;")
            return [x[0] for x in result]
        except ProgrammingError as e:
            print(f"MySQL ProgrammingError {e}")
            return []

    def _read_last_block(self) -> None | Dict[str, Any]:
        retval = database.query("SELECT * FROM blocks WHERE height = (SELECT MAX(height) FROM blocks);")
        return self._results_to_block(retval)

    def _read_last_block_as_hex(self) -> None | Dict[str, Any]:
        retval = database.query("SELECT * FROM blocks WHERE height = (SELECT MAX(height) FROM blocks);")
        return self._results_to_hex_blockheader(retval)

    def get_block_at_height(self, height: int) -> Dict[str, Any]:
        # Return the block at the given height
        block = self._read_block_from_height(height)
        if block is not None:
            # block = blockfile.load_at_offset(offset)
            txs = self._read_tx_at_height(block["height"])
            block["txs"] = txs
            return block
        else:
            return {
                "block": f"block height {height} not found",
            }

    def get_block_at_hash(self, hash: str) -> Dict[str, Any]:
        # Return the block at the given height
        block = self._read_block_from_hash(hash)
        if block is not None:
            txs = self._read_tx_at_height(block["height"])
            block["txs"] = txs
            return {
                "block": block,
            }
        else:
            return {
                "block": f"block hash {hash} not found",
            }

    def get_last_block(self) -> None | Dict[str, Any]:
        # Return the last block
        return self._read_last_block()

    def get_last_block_as_hex(self) -> None | Dict[str, Any]:
        # Return the last block
        return self._read_last_block_as_hex()

    def get_block_height(self) -> int:
        result = database.query("SELECT max(height) FROM blocks;")
        return result[0][0]

    def get_last_block_time(self) -> str:
        result = database.query("SELECT timestamp FROM blocks ORDER BY height desc LIMIT 1;")
        for x in result:
            retval = x
        timestamp = datetime.datetime.fromtimestamp(retval[0])
        return timestamp.strftime('%Y-%m-%d %H:%M:%S')


block_manager = BlockManager()
