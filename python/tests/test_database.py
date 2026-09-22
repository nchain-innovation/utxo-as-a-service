import pytest

from database import Database, connection_url


BASE = {
    "database": {
        "postgres_url": "postgresql://uaas:pw@localhost:5433/uaas_db",
        "postgres_url_docker": "postgresql://uaas:pw@postgres:5432/uaas_db",
    }
}


@pytest.fixture(autouse=True)
def _clear_url_env(monkeypatch):
    """Keep a real UAAS_POSTGRES_URL out of these tests.

    `connection_url` prefers that variable, so a developer with it exported —
    or CI, which sets it for the Rust suite — would otherwise see every
    config-reading test return the environment's URL instead of the one under
    test. Autouse so a test added later cannot forget.
    """
    monkeypatch.delenv("UAAS_POSTGRES_URL", raising=False)


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

    def test_db06_the_environment_url_wins_over_the_config(self, monkeypatch) -> None:
        # Mirrors Config::get_postgres_url on the Rust side. One setting has to
        # serve both components, or a deployment is back to editing a tracked
        # file to supply a credential (CS-450).
        monkeypatch.delenv("APP_ENV", raising=False)
        monkeypatch.setenv("UAAS_POSTGRES_URL", "postgresql://from-the-environment")
        assert connection_url(BASE) == "postgresql://from-the-environment"

    def test_db07_an_empty_environment_url_falls_back_to_the_config(
        self, monkeypatch
    ) -> None:
        # `UAAS_POSTGRES_URL=` in a shell, or an unset compose interpolation.
        monkeypatch.delenv("APP_ENV", raising=False)
        monkeypatch.setenv("UAAS_POSTGRES_URL", "")
        assert connection_url(BASE) == BASE["database"]["postgres_url"]

    def test_db08_a_placeholder_url_is_refused_and_says_what_to_set(
        self, monkeypatch
    ) -> None:
        monkeypatch.delenv("APP_ENV", raising=False)
        placeholder = {
            "database": {
                "postgres_url": "postgresql://uaas:CHANGE-ME@localhost:5433/uaas_db"
            }
        }
        with pytest.raises(RuntimeError, match="UAAS_POSTGRES_URL"):
            connection_url(placeholder)


class TestUnconfiguredPool:
    def test_db05_querying_before_set_config_is_an_error(self) -> None:
        with pytest.raises(RuntimeError, match="not configured"):
            Database().query("SELECT 1")
