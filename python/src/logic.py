import datetime
from typing import Dict, Any, MutableMapping
from mysql.connector import connect


class Logic:
    def __init__(self):
        self.host: str
        self.user: str
        self.password: str
        self.database: str
        self.network: str

    def set_config(self, config: MutableMapping[str, Any]):
        self.host = config["python"]["host"]
        self.user = config["python"]["user"]
        self.password = config["python"]["password"]
        self.database = config["python"]["database"]
        self.network = config['service']['network']

    def _get_last_block_time(self) -> str:
        with connect(
            host=self.host,
            user=self.user,
            password=self.password,
            database=self.database,
        ) as connection:
            query = ("SELECT timestamp FROM blocks ORDER BY height desc LIMIT 1;")
            cursor = connection.cursor()
            cursor.execute(query)
            for x in cursor:
                retval = x
            timestamp = datetime.datetime.fromtimestamp(retval[0])
            return timestamp.strftime('%Y-%m-%d %H:%M:%S')

    def _get_no_of_entries(self, provided_query: str) -> int:
        with connect(
            host=self.host,
            user=self.user,
            password=self.password,
            database=self.database,
        ) as connection:
            query = (provided_query)
            cursor = connection.cursor()
            cursor.execute(query)
            for x in cursor:
                retval = x
            return retval[0]

    def get_status(self) -> Dict[str, Dict[str, Any]]:
        return {
            "status": {
                "network": self.network,
                'last_block_time': self._get_last_block_time(),
                'number_of_blocks': self._get_no_of_entries("SELECT COUNT(*) FROM blocks;"),
                'number_of_tx': self._get_no_of_entries("SELECT COUNT(*) FROM tx;"),
                'number_of_utxo': self._get_no_of_entries("SELECT COUNT(*) FROM utxo;"),
                'number_of_mempool': self._get_no_of_entries("SELECT COUNT(*) FROM mempool;"),
            }
        }


logic = Logic()


def test_database():
    config = {
        "python":
            {
                "host": "host.docker.internal",
                "user": "uaas",
                "password": "uaas-password",
                "database": "uaas_db",
            }
    }

    logic.set_config(config)
    retval = logic.get_status()
    print(f"retval = {retval}")


if __name__ == '__main__':
    test_database()
