from typing import Any, List, Optional, Sequence

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

    def query(
        self,
        query_string: str,
        params: Optional[Sequence[Any]] = None,
    ) -> List[Any]:
        with connect(
            host=self.host,
            user=self.user,
            password=self.password,
            database=self.database,
        ) as connection:
            cursor = connection.cursor()
            cursor.execute(query_string, params or ())
            retval = list(cursor.fetchall())
            connection.commit()
            cursor.close()
            connection.close()
            return retval


database = Database()
