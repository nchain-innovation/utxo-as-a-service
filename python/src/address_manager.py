from typing import List, Dict, Any, MutableMapping
from mysql.connector import connect


class AddressManager:
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

    def _read_peers(self) -> List[Dict[str, Any]]:
        # Read peers from database
        with connect(
            host=self.host,
            user=self.user,
            password=self.password,
            database=self.database,
        ) as connection:
            query = ("SELECT * FROM addr")
            cursor = connection.cursor()
            cursor.execute(query)
            retval = []
            for x in cursor:
                retval.append({"ip": f"{x[0]}", "services": x[1], "port": x[2]})
            return retval

    def get_peers(self) -> Dict[str, List[Dict[str, Any]]]:
        """ Return a dictionary of addresses"""
        return {
            "peers": self._read_peers(),
        }


address_manager = AddressManager()


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

    address_manager.set_config(config)
    retval = address_manager.get_peers()
    print(f"retval = {retval}")


if __name__ == '__main__':
    test_database()
