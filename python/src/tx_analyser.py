from typing import List, Dict, Any, MutableMapping
from mysql.connector import connect


class TxAnalyser:
    def __init__(self):
        self.host: str
        self.user: str
        self.password: str
        self.database: str

    def set_config(self, config: MutableMapping[str, Any]):
        self.host = config["python"]["host"]
        self.user = config["python"]["user"]
        self.password = config["python"]["password"]
        self.database = config["python"]["database"]

    def _read_mempool(self) -> List[Dict[str, Any]]:
        # Read mempool from database
        with connect(
            host=self.host,
            user=self.user,
            password=self.password,
            database=self.database,
        ) as connection:
            query = ("SELECT * FROM mempool")
            cursor = connection.cursor()
            cursor.execute(query)
            retval = []
            for x in cursor:
                retval.append({"hash": f"{x[0]}", "locktime": x[1], "fee": x[2], "time": x[3], })
            return retval

    def get_mempool(self) -> Dict[str, List[Dict[str, Any]]]:
        """ Return a dictionary of mempool"""
        return {
            "mempool": self._read_mempool(),
        }


tx_analyser = TxAnalyser()


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

    tx_analyser.set_config(config)
    retval = tx_analyser.get_mempool()
    print(f"retval = {retval}")


if __name__ == '__main__':
    test_database()
