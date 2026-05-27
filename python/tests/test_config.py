import textwrap

import pytest

from config import ConfigError, load_config


def _write_config(tmp_path, content: str) -> str:
    path = tmp_path / "uaasr.toml"
    path.write_text(textwrap.dedent(content), encoding="utf-8")
    return str(path)


MINIMAL_CONFIG = """
[service]
user_agent = "/Bitcoin SV:1.0.11/"
network = "testnet"
rust_address = "127.0.0.1:8081"

[mainnet]
ip = ["127.0.0.1"]
port = 8333
start_block_hash = "{hash}"
start_block_height = 1
timeout_period = 60.0
startup_load_from_database = true
host = "127.0.0.1"
user = "maas"
password = "secret"
database = "main_uaas_db"
block_file = "../data/main-block.dat"
save_blocks = false
save_txs = false

[testnet]
ip = ["127.0.0.1", "127.0.0.2"]
port = 18333
start_block_hash = "{hash}"
start_block_height = 1
timeout_period = 60.0
startup_load_from_database = false
host = "127.0.0.1"
user = "uaas"
password = "secret"
database = "uaas_db"
block_file = "../data/test-net.dat"
save_blocks = false
save_txs = false

[database]
mysql_url = "mysql://local"
mysql_url_docker = "mysql://docker"
ms_delay = 300
retries = 3

[orphan]
detect = false
threshold = 100

[logging]
level = "info"

[utxo]
complete = 6

[dynamic_config]
filename = "../data/dynamic.toml"

[[collection]]
name = "demo"
track_descendants = false
address = "mgzhRq55hEYFgyCrtNxEsP1MdusZZ31hH5"

[web_interface]
address = "127.0.0.1:5010"
log_level = "info"
reload = false
rust_url = "http://127.0.0.1:8081"
"""


class TestConfigRequirements:
    def test_cfg01_loads_valid_toml(self, tmp_path) -> None:
        path = _write_config(tmp_path, MINIMAL_CONFIG.format(hash="a" * 64))
        config = load_config(path)
        assert config["service"]["network"] == "testnet"

    def test_cfg02_rejects_missing_service_section(self, tmp_path) -> None:
        path = _write_config(
            tmp_path,
            """
            [web_interface]
            address = "127.0.0.1:5010"
            log_level = "info"
            reload = false
            rust_url = "http://127.0.0.1:8081"
            """,
        )
        with pytest.raises(ConfigError, match="missing required section \\[service\\]"):
            load_config(path)

    def test_cfg02_rejects_invalid_rate_limit(self, tmp_path) -> None:
        content = MINIMAL_CONFIG.format(hash="a" * 64).replace(
            'rust_url = "http://127.0.0.1:8081"',
            'rust_url = "http://127.0.0.1:8081"\nrate_limit_per_minute = -1',
        )
        path = _write_config(tmp_path, content)
        with pytest.raises(ConfigError, match="rate_limit_per_minute"):
            load_config(path)

    def test_cfg04_reads_active_network_settings(self, tmp_path) -> None:
        path = _write_config(tmp_path, MINIMAL_CONFIG.format(hash="a" * 64))
        config = load_config(path)
        assert config["testnet"]["port"] == 18333
        assert config["mainnet"]["port"] == 8333
        assert len(config["testnet"]["ip"]) == 2
        assert config["testnet"]["startup_load_from_database"] is False

    def test_cfg05_loads_static_collections(self, tmp_path) -> None:
        path = _write_config(tmp_path, MINIMAL_CONFIG.format(hash="a" * 64))
        config = load_config(path)
        assert config["collection"][0]["name"] == "demo"

    def test_sync07_orphan_detection_flag_is_configurable(self, tmp_path) -> None:
        content = MINIMAL_CONFIG.format(hash="a" * 64).replace(
            "detect = false",
            "detect = true",
        )
        path = _write_config(tmp_path, content)
        config = load_config(path)
        assert config["orphan"]["detect"] is True
