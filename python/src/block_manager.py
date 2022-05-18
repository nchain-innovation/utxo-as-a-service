from typing import List, Dict, Any, Optional, MutableMapping
import time
from database import database
from blockfile import blockfile


class BlockManager:
    def __init__(self):
        self.start_block_height: int

    def set_config(self, config: MutableMapping[str, Any]):
        network = config['service']['network']
        self.start_block_height = config[network]['start_block_height']

    def _read_latest_blocks(self) -> List[Dict[str, Any]]:
        # Read blocks from database
        result = database.query("SELECT * FROM blocks ORDER BY height desc LIMIT 20;")
        retval = []
        for x in result:
            retval.append({
                "height": x[0] + self.start_block_height,
                "hash": x[1],
                "version": f'{x[2]:08x}',
                "prev_hash": x[3],
                "merkle_root": x[4],
                "timestamp": time.ctime(x[5]),
                "bits": f'{x[6]:08x}',
                "nonce": f'{x[7]:08x}',
                "offset": f'{x[8]:08x}'
            })
        return retval

    def get_latest_blocks(self) -> Dict[str, List[Dict[str, Any]]]:
        """ Return a dictionary of blocks"""
        return {
            "blocks": self._read_latest_blocks(),
        }

    def _read_block_offset(self, height: int) -> Optional[int]:
        # Read block from database
        h1 = height - self.start_block_height
        retval = database.query(f"SELECT offset FROM blocks WHERE height = '{h1}';")
        if retval != []:
            return int(retval[0][0])
        else:
            return None

    def get_block(self, height: int) -> Dict[str, Any]:
        # Return the block at the given height
        offset = self._read_block_offset(height)
        if offset is not None:
            block = blockfile.load_at_offset(offset)
            return {
                "block": block.to_dict(),
            }
        else:
            return {
                "block": f"block height {height} not found",
            }

    def _read_block_offset_from_hash(self, hash: str) -> Optional[int]:
        # Read block from database
        retval = database.query(f"SELECT offset FROM blocks WHERE hash = '{hash}';")
        if retval != []:
            return int(retval[0][0])
        else:
            return None

    def get_block_at_hash(self, hash: str) -> Dict[str, Any]:
        # Return the block at the given height
        offset = self._read_block_offset_from_hash(hash)
        if offset is not None:
            block = blockfile.load_at_offset(offset)
            return {
                "block": block.to_dict(),
            }
        else:
            return {
                "block": f"block hash {hash} not found",
            }


block_manager = BlockManager()
