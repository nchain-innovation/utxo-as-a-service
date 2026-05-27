from unittest.mock import MagicMock, patch

from logic import Logic


class TestLogic:
    def test_get_no_of_entries_returns_zero_on_missing_table(self) -> None:
        from mysql.connector.errors import ProgrammingError

        logic = Logic()
        with patch(
            "logic.database.query",
            side_effect=ProgrammingError("Table does not exist"),
        ):
            assert logic._get_no_of_entries("SELECT COUNT(*) FROM tx;") == 0

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
        response.json.return_value = {"version": "1.2.0"}
        with patch("logic.requests.get", return_value=response):
            assert logic._get_version() == "1.2.0"

    def test_get_status_includes_database_counts(self) -> None:
        logic = Logic()
        logic.network = "testnet"
        logic.rust_url = "http://127.0.0.1:8081"

        with patch("logic.block_manager.get_block_height", return_value=10), patch(
            "logic.block_manager.get_last_block_time",
            return_value="2024-01-01 00:00:00",
        ), patch.object(logic, "_get_version", return_value="1.2.0"), patch.object(
            logic,
            "_get_no_of_entries",
            side_effect=[5, 3, 1],
        ):
            status = logic.get_status()

        assert status == {
            "network": "testnet",
            "version": "1.2.0",
            "last block time": "2024-01-01 00:00:00",
            "block height": 10,
            "number of txs": 5,
            "number of utxo entries": 3,
            "number of mempool entries": 1,
        }
