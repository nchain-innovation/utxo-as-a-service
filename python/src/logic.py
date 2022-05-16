import datetime
from typing import Dict, Any, MutableMapping

from database import database


class Logic:
    def __init__(self):
        self.network: str

    def set_config(self, config: MutableMapping[str, Any]):
        self.network = config['service']['network']

    def _get_last_block_time(self) -> str:
        result = database.query("SELECT timestamp FROM blocks ORDER BY height desc LIMIT 1;")
        for x in result:
            retval = x
        timestamp = datetime.datetime.fromtimestamp(retval[0])
        return timestamp.strftime('%Y-%m-%d %H:%M:%S')

    def _get_no_of_entries(self, provided_query: str) -> int:
        result = database.query(provided_query)
        return result[0][0]

    def get_status(self) -> Dict[str, Dict[str, Any]]:
        return {
            "status": {
                "network": self.network,
                'last block time': self._get_last_block_time(),
                'number of blocks': self._get_no_of_entries("SELECT COUNT(*) FROM blocks;"),
                'number of txs': self._get_no_of_entries("SELECT COUNT(*) FROM tx;"),
                'number of utxo entries': self._get_no_of_entries("SELECT COUNT(*) FROM utxo;"),
                'number of mempool entries': self._get_no_of_entries("SELECT COUNT(*) FROM mempool;"),
            }
        }


logic = Logic()
