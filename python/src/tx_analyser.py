import datetime
from typing import List, Dict, Any, Optional

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

    def _read_block_offset(self, hash: str) -> Optional[int]:
        # Read block offset based on tx hash from database
        result = database.query(
            f"SELECT offset FROM uaas_db.blocks INNER JOIN uaas_db.tx on uaas_db.tx.height = uaas_db.blocks.height where uaas_db.tx.hash='{hash}';")
        try:
            return result[0][0]
        except IndexError:
            return None

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
        tx_as_dict = tx.to_dict()

        # Get utxo
        utxo_entry = self._read_utxo(hash)
        # Create a list of unspent pos
        utxo = list(map(lambda x: x['pos'], utxo_entry))
        for pos, vout in enumerate(tx_as_dict['vout']):
            vout["spent"] = pos not in utxo

        return {
            "tx": tx_as_dict,
        }

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


tx_analyser = TxAnalyser()
