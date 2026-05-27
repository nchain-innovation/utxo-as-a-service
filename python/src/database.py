from typing import Any, List, Optional, Sequence

from mysql.connector.pooling import MySQLConnectionPool

from config import ConfigType

_WRITE_PREFIXES = (
    "INSERT",
    "UPDATE",
    "DELETE",
    "REPLACE",
    "CREATE",
    "DROP",
    "ALTER",
    "TRUNCATE",
    "GRANT",
    "REVOKE",
)


def _requires_commit(query_string: str) -> bool:
    """Return True when the statement mutates database state."""
    normalized = query_string.lstrip()
    if not normalized:
        return False
    first_token = normalized.split(None, 1)[0].upper()
    return first_token in _WRITE_PREFIXES


class Database:
    def __init__(self):
        self._pool: MySQLConnectionPool | None = None

    def set_config(self, config: ConfigType):
        network = config["service"]["network"]
        network_config = config[network]
        pool_kwargs: dict[str, Any] = {
            "pool_name": "uaas_pool",
            "pool_size": 5,
            "pool_reset_session": True,
            "host": network_config["host"],
            "user": network_config["user"],
            "password": network_config["password"],
            "database": network_config["database"],
        }
        mysql_port = network_config.get("mysql_port")
        if mysql_port is not None:
            pool_kwargs["port"] = mysql_port
        self._pool = MySQLConnectionPool(**pool_kwargs)

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
                if _requires_commit(query_string):
                    connection.commit()
                return retval
            finally:
                cursor.close()
        finally:
            connection.close()


database = Database()
