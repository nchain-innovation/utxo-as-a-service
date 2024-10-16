import datetime
from typing import Dict, Any, MutableMapping

import requests
from database import database


class Logic:
    def __init__(self):
        self.network: str
        self.start_block_height: int
        self.rust_url: str

    def set_config(self, config: MutableMapping[str, Any]):
        self.network = config['service']['network']
        self.rust_url = config["web_interface"]["rust_url"]

    def _get_last_block_time(self) -> str:
        result = database.query("SELECT timestamp FROM blocks ORDER BY height desc LIMIT 1;")
        for x in result:
            retval = x
        timestamp = datetime.datetime.fromtimestamp(retval[0])
        return timestamp.strftime('%Y-%m-%d %H:%M:%S')

    def _get_no_of_entries(self, provided_query: str) -> int:
        result = database.query(provided_query)
        return result[0][0]

    def _get_version(self) -> str:
        url = self.rust_url + "/version"
        try:
            result = requests.get(url)
        except requests.exceptions.ConnectionError as e:
            print(f"failure = {str(e)}, url = {url}")
        else:
            if result.status_code == 200:
                return result.json()["version"]
        return "unknown"

    def get_status(self) -> Dict[str, Dict[str, Any]]:
        block_height = self._get_no_of_entries("SELECT max(height) FROM blocks;")
        return {
            "status": {
                "network": self.network,
                "version": self._get_version(),
                'last block time': self._get_last_block_time(),
                'block height': block_height,
                'number of txs': self._get_no_of_entries("SELECT COUNT(*) FROM tx;"),
                'number of utxo entries': self._get_no_of_entries("SELECT COUNT(*) FROM utxo;"),
                'number of mempool entries': self._get_no_of_entries("SELECT COUNT(*) FROM mempool;"),
            }
        }


logic = Logic()
