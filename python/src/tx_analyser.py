import datetime
from typing import List, Dict, Any

from database import database
from blockfile import blockfile


class TxAnalyser:
    def _read_mempool(self) -> List[Dict[str, Any]]:
        # Read mempool from database
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
        # Read mempool from database
        result = database.query(f"SELECT * FROM utxo WHERE hash = '{hash}';")
        retval = [{
            "hash": f"{x[0]}", "pos": x[1], "satoshi": x[2], "height": x[3]
        } for x in result]
        return retval

    def get_utxo_entry(self, hash: str) -> Dict[str, List[Dict[str, Any]]]:
        """ Return the utxo entry identified by hash"""
        return {
            "utxo": self._read_utxo(hash),
        }

    def _read_block_offset(self, hash: str) -> int:
        # Read block offset based on tx hash from database
        result = database.query(
            f"SELECT offset FROM uaas_db.blocks INNER JOIN uaas_db.tx on uaas_db.tx.height = uaas_db.blocks.height where uaas_db.tx.hash='{hash}';")
        return result[0][0]

    def get_tx_entry(self, hash: str) -> Dict[str, Dict[str, Any]]:
        """ Return the utxo entry identified by hash"""
        offset = self._read_block_offset(hash)
        block = blockfile.load_at_offset(offset)
        tx = list(filter(lambda x: x.hash == hash, block.vtx))[0]
        return {
            "tx": tx.to_dict(),
        }


tx_analyser = TxAnalyser()
