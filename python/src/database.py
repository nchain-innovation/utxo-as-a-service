from typing import Any, List, Optional, Sequence

from mysql.connector.pooling import MySQLConnectionPool

from config import ConfigType


class Database:
    def __init__(self):
        self._pool: MySQLConnectionPool | None = None

    def set_config(self, config: ConfigType):
        network = config["service"]["network"]
        self._pool = MySQLConnectionPool(
            pool_name="uaas_pool",
            pool_size=5,
            pool_reset_session=True,
            host=config[network]["host"],
            user=config[network]["user"],
            password=config[network]["password"],
            database=config[network]["database"],
        )

    def query(
        self,
        query_string: str,
        params: Optional[Sequence[Any]] = None,
    ) -> List[Any]:
        if self._pool is None:
            raise RuntimeError("Database pool is not configured")

        connection = self._pool.get_connection()
        try:
            cursor = connection.cursor()
            try:
                cursor.execute(query_string, params or ())
                retval = list(cursor.fetchall())
                connection.commit()
                return retval
            finally:
                cursor.close()
        finally:
            connection.close()


database = Database()
