from unittest.mock import MagicMock, patch

import pytest

from logic import Logic


class TestLogic:
    def test_get_no_of_entries_propagates_a_database_error(self) -> None:
        # This used to swallow a missing-table error and report zero entries.
        # The schema is versioned now and the service refuses to start against
        # a version it does not expect, so a schema fault here is a real fault:
        # reporting it as "zero rows" hides a broken deployment behind a
        # plausible number.
        import psycopg

        logic = Logic()
        with patch(
            "logic.database.query",
            side_effect=psycopg.errors.UndefinedTable("relation \"tx\" does not exist"),
        ):
            with pytest.raises(psycopg.errors.UndefinedTable):
                logic._get_no_of_entries("SELECT COUNT(*) FROM tx;")

    def test_get_no_of_entries_returns_the_count(self) -> None:
        logic = Logic()
        with patch("logic.database.query", return_value=[(42,)]):
            assert logic._get_no_of_entries("SELECT COUNT(*) FROM tx;") == 42

    def test_get_version_returns_unknown_when_rust_unreachable(self) -> None:
        import requests

        logic = Logic()
        logic.rust_url = "http://127.0.0.1:59999"
        with patch(
            "logic.requests.get",
            side_effect=requests.exceptions.ConnectionError("connection refused"),
        ):
            assert logic._get_version() == "unknown"

    def test_get_version_returns_version_from_rust(self) -> None:
        logic = Logic()
        logic.rust_url = "http://127.0.0.1:8081"
        response = MagicMock()
        response.status_code = 200
        response.json.return_value = {"version": "1.3.0"}
        with patch("logic.requests.get", return_value=response):
            assert logic._get_version() == "1.3.0"

    def test_get_status_includes_database_counts(self) -> None:
        logic = Logic()
        logic.network = "testnet"
        logic.rust_url = "http://127.0.0.1:8081"

        with patch("logic.block_manager.get_block_height", return_value=10), patch(
            "logic.block_manager.get_last_block_time",
            return_value="2024-01-01 00:00:00",
        ), patch.object(logic, "_get_version", return_value="1.3.0"), patch.object(
            logic,
            "_get_no_of_entries",
            side_effect=[5, 3, 1],
        ):
            status = logic.get_status()

        assert status == {
            "network": "testnet",
            "version": "1.3.0",
            "last block time": "2024-01-01 00:00:00",
            "block height": 10,
            "number of txs": 5,
            "number of utxo entries": 3,
            "number of mempool entries": 1,
        }
