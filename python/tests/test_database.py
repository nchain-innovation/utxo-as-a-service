import pytest

from database import Database, connection_url


BASE = {
    "database": {
        "postgres_url": "postgresql://uaas:pw@localhost:5433/uaas_db",
        "postgres_url_docker": "postgresql://uaas:pw@postgres:5432/uaas_db",
    }
}


class TestConnectionUrl:
    def test_db01_uses_the_host_url_by_default(self, monkeypatch) -> None:
        monkeypatch.delenv("APP_ENV", raising=False)
        assert connection_url(BASE) == BASE["database"]["postgres_url"]

    def test_db02_uses_the_docker_url_when_app_env_is_set(self, monkeypatch) -> None:
        # Matches how the Rust service chooses, so the two components cannot
        # end up pointing at different databases. That is CS-407.
        monkeypatch.setenv("APP_ENV", "docker")
        assert connection_url(BASE) == BASE["database"]["postgres_url_docker"]

    def test_db03_a_missing_database_section_is_an_error(self, monkeypatch) -> None:
        monkeypatch.delenv("APP_ENV", raising=False)
        with pytest.raises(RuntimeError, match=r"\[database\]"):
            connection_url({"service": {}})

    def test_db04_a_missing_key_is_an_error(self, monkeypatch) -> None:
        monkeypatch.delenv("APP_ENV", raising=False)
        with pytest.raises(RuntimeError, match="postgres_url"):
            connection_url({"database": {}})


class TestUnconfiguredPool:
    def test_db05_querying_before_set_config_is_an_error(self) -> None:
        with pytest.raises(RuntimeError, match="not configured"):
            Database().query("SELECT 1")
