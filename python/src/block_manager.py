from typing import List, Dict, Any, Optional
import datetime

from database import database
from blockfile import blockfile


class BlockManager:

    def _read_latest_blocks(self) -> List[Dict[str, Any]]:
        # Read blocks from database
        result = database.query("SELECT * FROM blocks ORDER BY height desc LIMIT 20;")
        retval = []
        for x in result:
            timestamp = datetime.datetime.fromtimestamp(x[5])
            retval.append({
                "height": x[0], "hash": x[1], "version": x[2], "prev_hash": x[3], "merkle_root": x[4],
                "timestamp": timestamp.strftime('%Y-%m-%d %H:%M:%S'),
                "bits": x[6], "nonce": x[7], "offset": x[8]
            })
        return retval

    def get_latest_blocks(self) -> Dict[str, List[Dict[str, Any]]]:
        """ Return a dictionary of blocks"""
        return {
            "blocks": self._read_latest_blocks(),
        }

    def _read_block_offset(self, height: int) -> Optional[int]:
        # Read block from database
        retval = database.query(f"SELECT offset FROM blocks WHERE height = '{height}';")
        print(f"retval = {retval}")
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
        print(f"retval = {retval}")
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
