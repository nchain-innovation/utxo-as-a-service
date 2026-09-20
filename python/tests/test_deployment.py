from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
COMPOSE_FILE = REPO_ROOT / "docker-compose.yml"

# The two MariaDB tests that used to live here — that compose mounted
# docker/mariadb/99-uaas.cnf, and that the file set innodb_buffer_pool_size —
# are gone with the engine. They asserted the presence of tuning for a database
# nothing speaks.


class TestDeploymentRequirements:
    def test_ops01_compose_defines_all_services(self) -> None:
        content = COMPOSE_FILE.read_text(encoding="utf-8")
        for service in (
            "postgres:",
            "uaas_migrate:",
            "adminer:",
            "uaas_backend:",
            "uaas_web:",
        ):
            assert service in content, f"{service} missing from docker-compose.yml"
        assert "uaas_network:" in content

    def test_ops02_database_has_healthcheck(self) -> None:
        content = COMPOSE_FILE.read_text(encoding="utf-8")
        assert "pg_isready" in content

    def test_ops10_the_schema_is_applied_before_the_services_start(self) -> None:
        # The backend refuses to start against a schema version it does not
        # recognise, so the one-shot migrate service has to have completed
        # first. Nothing else applies the schema: the init script mounted into
        # the postgres container only runs on an empty data directory.
        content = COMPOSE_FILE.read_text(encoding="utf-8")
        assert "service_completed_successfully" in content

    def test_ops11_no_mysql_family_engine_remains(self) -> None:
        content = COMPOSE_FILE.read_text(encoding="utf-8").lower()
        for token in ("mariadb", "mysql"):
            assert token not in content, f"{token} still referenced in compose"

    def test_ops03_backend_healthcheck_targets_rust_health(self) -> None:
        content = COMPOSE_FILE.read_text(encoding="utf-8")
        assert "8081/health" in content

    def test_ops04_web_healthcheck_targets_python_health(self) -> None:
        content = COMPOSE_FILE.read_text(encoding="utf-8")
        assert "5010/health" in content

    def test_ops05_application_services_mount_shared_data(self) -> None:
        content = COMPOSE_FILE.read_text(encoding="utf-8")
        assert "./data:/app/data" in content
        assert "uaasr.docker.toml:/app/data/uaasr.toml" in content
