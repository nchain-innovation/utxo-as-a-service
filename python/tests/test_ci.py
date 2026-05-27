from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
CI_FILE = REPO_ROOT / ".github" / "workflows" / "ci.yml"


class TestCiRequirements:
    def test_ops07_ci_runs_rust_checks(self) -> None:
        content = CI_FILE.read_text(encoding="utf-8")
        assert "cargo fmt --check" in content
        assert "cargo clippy" in content
        assert "cargo test" in content

    def test_ops08_ci_runs_python_checks(self) -> None:
        content = CI_FILE.read_text(encoding="utf-8")
        assert "./lint.sh" in content
        assert "pytest python/tests" in content

    def test_ops09_ci_builds_docker_images(self) -> None:
        content = CI_FILE.read_text(encoding="utf-8")
        assert "./build.sh" in content
