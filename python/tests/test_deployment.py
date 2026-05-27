from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
COMPOSE_FILE = REPO_ROOT / "docker-compose.yml"
MARIADB_CNF = REPO_ROOT / "docker" / "mariadb" / "99-uaas.cnf"


class TestDeploymentRequirements:
    def test_compose_mounts_mariadb_innodb_tuning(self) -> None:
        content = COMPOSE_FILE.read_text(encoding="utf-8")
        assert "docker/mariadb/99-uaas.cnf" in content
        assert MARIADB_CNF.is_file()

    def test_mariadb_cnf_sets_innodb_buffer_pool(self) -> None:
        content = MARIADB_CNF.read_text(encoding="utf-8")
        assert "innodb_buffer_pool_size" in content
        assert "innodb_flush_log_at_trx_commit = 2" in content
