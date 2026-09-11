import logging
from typing import Dict, Any

import requests
from database import database
from block_manager import block_manager
from config import ConfigType
from mysql.connector.errors import ProgrammingError

RUST_REQUEST_TIMEOUT = 30  # seconds
LOGGER = logging.getLogger(__name__)


class Logic:
    def __init__(self):
        self.network: str
        self.start_block_height: int
        self.rust_url: str

    def set_config(self, config: ConfigType):
        self.network = config['service']['network']
        self.rust_url = config["web_interface"]["rust_url"]

    def _get_no_of_entries(self, provided_query: str) -> int:
        try:
            result = database.query(provided_query)
            return result[0][0]
        except ProgrammingError as e:
            LOGGER.error("MySQL ProgrammingError: %s", e)
            return 0

    def _get_version(self) -> str:
        url = self.rust_url + "/version"
        try:
            result = requests.get(url, timeout=RUST_REQUEST_TIMEOUT)
        except requests.exceptions.Timeout:
            LOGGER.warning("Timeout requesting version from %s", url)
        except requests.exceptions.ConnectionError as e:
            LOGGER.warning("Unable to connect to %s: %s", url, e)
        else:
            if result.status_code == 200:
                return result.json()["version"]
        return "unknown"

    def get_status(self) -> Dict[str, Any]:
        block_height = block_manager.get_block_height()
        last_block_time = block_manager.get_last_block_time()
        return {
            "network": self.network,
            "version": self._get_version(),
            'last block time': last_block_time,
            'block height': block_height,
            'number of txs': self._get_no_of_entries("SELECT COUNT(*) FROM tx;"),
            'number of utxo entries': self._get_no_of_entries("SELECT COUNT(*) FROM utxo;"),
            'number of mempool entries': self._get_no_of_entries("SELECT COUNT(*) FROM mempool;"),
        }


logic = Logic()
