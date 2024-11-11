from typing import Any, List

from mysql.connector import connect
from config import ConfigType


class Database:
    def __init__(self):
        self.host: str
        self.user: str
        self.password: str
        self.database: str

    def set_config(self, config: ConfigType):
        network = config['service']['network']
        self.host = config[network]["host"]
        self.user = config[network]["user"]
        self.password = config[network]["password"]
        self.database = config[network]["database"]

    def query(self, query_string: str) -> List[Any]:
        with connect(
            host=self.host,
            user=self.user,
            password=self.password,
            database=self.database,
        ) as connection:
            query = (query_string)
            cursor = connection.cursor()
            cursor.execute(query)
            retval = []
            for x in cursor:
                retval.append(x)
            connection.commit()
            cursor.close()
            connection.close()
            return retval


database = Database()
