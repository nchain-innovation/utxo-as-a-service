import textwrap
from unittest.mock import MagicMock, patch

import pytest
from fastapi.testclient import TestClient

from collection import Monitor, collection, load_dynamic_config


TESTNET_ADDRESS = "mgzhRq55hEYFgyCrtNxEsP1MdusZZ31hH5"


class TestCollectionRequirements:
    def test_cfg06_loads_dynamic_monitors_from_file(self, tmp_path) -> None:
        dynamic_file = tmp_path / "dynamic.toml"
        dynamic_file.write_text(
            textwrap.dedent(
                """
                [[collection]]
                name = "runtime-monitor"
                track_descendants = false
                address = "mgzhRq55hEYFgyCrtNxEsP1MdusZZ31hH5"
                """
            ),
            encoding="utf-8",
        )
        config = {
            "dynamic_config": {"filename": str(dynamic_file)},
            "collection": [],
        }
        assert load_dynamic_config(config) == ["runtime-monitor"]

    def test_api08_returns_tx_from_collection_when_not_save_blocks(
        self,
        client: TestClient,
    ) -> None:
        import rest_api

        tx_hash = "a" * 64
        with patch.object(rest_api.collection, "get_tx_as_hex", return_value=[("0100000001",)]):
            response = client.get("/tx/hex", params={"hash": tx_hash})
        assert response.status_code == 200
        assert response.json()["result"] == "0100000001"

    def test_sync10_collection_stores_monitor_names_in_memory(self, tmp_path) -> None:
        import rest_api

        dynamic_file = tmp_path / "dynamic.toml"
        dynamic_file.write_text("collection = []\n", encoding="utf-8")
        rest_api.collection.set_config(
            {
                "collection": [],
                "dynamic_config": {"filename": str(dynamic_file)},
            }
        )
        monitor = Monitor(
            name="temp-monitor-xyz",
            track_descendants=False,
            address=TESTNET_ADDRESS,
            locking_script_pattern=None,
        )
        assert not rest_api.collection.is_valid_collection(monitor.name)
        rest_api.collection.add_monitor(monitor)
        assert rest_api.collection.is_valid_collection(monitor.name)
        rest_api.collection.delete_monitor(monitor.name)
