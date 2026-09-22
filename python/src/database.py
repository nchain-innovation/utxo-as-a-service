import os
from typing import Any, List, Optional, Sequence

from psycopg_pool import ConnectionPool

from config import ConfigType


CREDENTIAL_PLACEHOLDER = "CHANGE-ME"


def connection_url(config: ConfigType) -> str:
    """The libpq URL for this environment.

    `UAAS_POSTGRES_URL` wins over the config file, matching
    `Config::get_postgres_url` in the Rust service so one setting serves both
    components and neither needs a credential in a tracked file. This
    repository is public (CS-450). An empty value counts as unset.

    Falling back, read from `[database]`, the same two keys the Rust service
    reads, chosen the same way — `APP_ENV=docker` selects the in-container
    host. Before this, the two components were configured separately: Rust from
    a URL and Python from discrete host/port/user/password/database keys
    repeated under every network section. Nothing kept them pointing at the
    same database, which is what CS-407 was about.
    """
    env_url = os.environ.get("UAAS_POSTGRES_URL")
    if env_url:
        return env_url

    try:
        database = config["database"]
    except KeyError as err:
        raise RuntimeError(
            "Config is missing the [database] section; it must set postgres_url "
            "and postgres_url_docker."
        ) from err

    key = "postgres_url_docker" if os.environ.get("APP_ENV") else "postgres_url"
    try:
        url = str(database[key])
    except KeyError as err:
        raise RuntimeError(f"Config section [database] is missing '{key}'.") from err

    # Refuse rather than connect. The tracked configs carry a placeholder, so
    # reaching here with one means nothing supplied a real URL.
    if CREDENTIAL_PLACEHOLDER in url:
        raise RuntimeError(
            f"Config section [database] '{key}' is still the "
            f"{CREDENTIAL_PLACEHOLDER} placeholder. Set UAAS_POSTGRES_URL, or "
            "point the config at one that is not tracked by git. Do not commit "
            "a real URL: this repository is public."
        )
    return url


class Database:
    def __init__(self) -> None:
        self._pool: ConnectionPool | None = None

    def set_config(self, config: ConfigType) -> None:
        # `open=True` connects eagerly, so a bad URL or an unreachable server
        # fails at startup rather than on the first request.
        self._pool = ConnectionPool(
            connection_url(config),
            min_size=1,
            max_size=5,
            open=True,
        )

    def close(self) -> None:
        """Shut the pool down.

        The process holds one pool for its lifetime, so production never needs
        this. Tests do: the pool runs background threads, and letting it be
        collected at interpreter shutdown raises PythonFinalizationError from
        its finaliser because those threads can no longer be joined.
        """
        if self._pool is not None:
            self._pool.close()
            self._pool = None

    def query(
        self,
        query_string: str,
        params: Optional[Sequence[Any]] = None,
    ) -> List[Any]:
        if self._pool is None:
            raise RuntimeError("Database pool is not configured")

        # psycopg commits when the `connection` block exits without an
        # exception and rolls back when it does not. That replaces the
        # `_requires_commit` helper this used to carry, which decided whether
        # to commit by matching the statement's first word against a list of
        # prefixes — a heuristic that got `WITH ... INSERT` wrong, and one more
        # thing to keep in step with the SQL.
        with self._pool.connection() as connection:
            with connection.cursor() as cursor:
                cursor.execute(query_string, params or ())
                # `description` is None for a statement that returns no rows.
                # Calling fetchall() on one raises in psycopg rather than
                # returning an empty list, as the MySQL connector did.
                if cursor.description is None:
                    return []
                return list(cursor.fetchall())


database = Database()
