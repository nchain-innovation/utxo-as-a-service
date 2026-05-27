from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
COMPOSE_FILE = REPO_ROOT / "docker-compose.yml"


class TestDeploymentRequirements:
    def test_ops01_compose_defines_all_services(self) -> None:
        content = COMPOSE_FILE.read_text(encoding="utf-8")
        for service in ("database:", "adminer:", "uaas_backend:", "uaas_web:"):
            assert service in content
        assert "uaas_network:" in content

    def test_ops02_database_has_healthcheck(self) -> None:
        content = COMPOSE_FILE.read_text(encoding="utf-8")
        assert "healthcheck.sh" in content
        assert "database:" in content

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
