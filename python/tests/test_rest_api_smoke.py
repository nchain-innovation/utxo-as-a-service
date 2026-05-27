from unittest.mock import MagicMock, patch

from fastapi.testclient import TestClient

VALID_HASH = "a" * 64
TESTNET_ADDRESS = "mgzhRq55hEYFgyCrtNxEsP1MdusZZ31hH5"
MINIMAL_TX_HEX = "0100000001"


class TestRestApiSmoke:
    def test_root(self, client: TestClient) -> None:
        response = client.get("/")
        assert response.status_code == 200
        assert response.json()["name"] == "UTXO as a Service (UaaS) REST API"

    def test_health_when_database_available(self, client: TestClient) -> None:
        import rest_api

        with patch.object(rest_api.database, "query", return_value=[(1,)]):
            response = client.get("/health")
        assert response.status_code == 200
        body = response.json()
        assert body["status"] == "ok"
        assert body["service"] == "uaas-web"

    def test_health_when_database_unavailable(self, client: TestClient) -> None:
        import rest_api

        with patch.object(
            rest_api.database,
            "query",
            side_effect=RuntimeError("connection refused"),
        ):
            response = client.get("/health")
        assert response.status_code == 503
        body = response.json()
        assert body["status"] == "unhealthy"
        assert body["service"] == "uaas-web"
        assert "connection refused" in body["database"]

    def test_health_does_not_require_api_key(self, client: TestClient) -> None:
        import rest_api

        with patch.object(rest_api, "api_key", "secret-key"):
            with patch.object(rest_api.database, "query", return_value=[(1,)]):
                response = client.get("/health")
        assert response.status_code == 200
        assert response.json()["status"] == "ok"

    def test_protected_endpoint_requires_api_key(self, client: TestClient) -> None:
        import rest_api

        with patch.object(rest_api, "api_key", "secret-key"):
            response = client.get("/status")
        assert response.status_code == 401
        assert response.json()["failure"] == "Unauthorized"

    def test_protected_endpoint_accepts_api_key(self, client: TestClient) -> None:
        import rest_api

        with patch.object(rest_api, "api_key", "secret-key"), patch.object(
            rest_api.logic,
            "get_status",
            return_value={"network": "testnet"},
        ):
            response = client.get("/status", headers={"X-API-Key": "secret-key"})
        assert response.status_code == 200
        assert response.json()["network"] == "testnet"

    def test_options_request_bypasses_api_key(self, client: TestClient) -> None:
        import rest_api

        with patch.object(rest_api, "api_key", "secret-key"):
            response = client.options("/status")
        assert response.status_code != 401

    def test_invalid_tx_hash_returns_422(self, client: TestClient) -> None:
        response = client.get("/tx", params={"hash": "not-a-hash"})
        assert response.status_code == 422
        assert "failure" in response.json()

    def test_unknown_tx_returns_422(self, client: TestClient) -> None:
        import rest_api

        with patch.object(rest_api.collection, "get_tx_as_hex", return_value=[]):
            response = client.get("/tx", params={"hash": VALID_HASH})
        assert response.status_code == 422
        assert "Unknown txid" in response.json()["failed"]

    def test_unknown_tx_hex_returns_422(self, client: TestClient) -> None:
        import rest_api

        with patch.object(rest_api.collection, "get_tx_as_hex", return_value=[]):
            response = client.get("/tx/hex", params={"hash": VALID_HASH})
        assert response.status_code == 422
        assert "Unknown txid" in response.json()["failed"]

    def test_invalid_block_height_returns_422(self, client: TestClient) -> None:
        response = client.get("/block/height", params={"height": -1})
        assert response.status_code == 422
        assert "failure" in response.json()

    def test_rate_limit_returns_429(self, client: TestClient) -> None:
        import rest_api
        from rate_limit import FixedWindowRateLimiter

        original_limit = rest_api.rate_limit_per_minute
        original_limiter = rest_api.rate_limiter
        rest_api.rate_limit_per_minute = 1
        rest_api.rate_limiter = FixedWindowRateLimiter(1)
        try:
            assert client.get("/").status_code == 200
            response = client.get("/")
            assert response.status_code == 429
            assert response.json()["failure"] == "Rate limit exceeded"
        finally:
            rest_api.rate_limit_per_minute = original_limit
            rest_api.rate_limiter = original_limiter

    def test_health_exempt_from_rate_limit(self, client: TestClient) -> None:
        import rest_api
        from rate_limit import FixedWindowRateLimiter

        original_limit = rest_api.rate_limit_per_minute
        original_limiter = rest_api.rate_limiter
        rest_api.rate_limit_per_minute = 1
        rest_api.rate_limiter = FixedWindowRateLimiter(1)
        try:
            with patch.object(rest_api.database, "query", return_value=[(1,)]):
                assert client.get("/health").status_code == 200
                assert client.get("/health").status_code == 200
        finally:
            rest_api.rate_limit_per_minute = original_limit
            rest_api.rate_limiter = original_limiter

    def test_oversized_broadcast_tx_returns_422(self, client: TestClient) -> None:
        import rest_api

        hexstr = "00" * (rest_api.max_broadcast_tx_bytes + 1)
        response = client.post("/tx/hex", json={"tx": hexstr})
        assert response.status_code == 422

    def test_invalid_block_hash_returns_422(self, client: TestClient) -> None:
        response = client.get("/block/hash", params={"hash": "not-a-hash"})
        assert response.status_code == 422
        assert "failure" in response.json()

    def test_block_last_returns_503_when_no_blocks(self, client: TestClient) -> None:
        import rest_api

        with patch.object(rest_api.block_manager, "get_last_block", return_value=None):
            response = client.get("/block/last")
        assert response.status_code == 503
        assert response.json() == {}

    def test_block_last_hex_returns_503_when_no_blocks(self, client: TestClient) -> None:
        import rest_api

        with patch.object(
            rest_api.block_manager,
            "get_last_block_as_hex",
            return_value=None,
        ):
            response = client.get("/block/last/hex")
        assert response.status_code == 503
        assert response.json() == {}

    def test_invalid_address_returns_422_for_utxo(self, client: TestClient) -> None:
        response = client.get("/utxo/get", params={"address": "not-an-address"})
        assert response.status_code == 422
        assert "failure" in response.json()

    def test_invalid_address_returns_422_for_balance(self, client: TestClient) -> None:
        response = client.get("/utxo/balance", params={"address": "not-an-address"})
        assert response.status_code == 422
        assert "failure" in response.json()

    def test_utxo_returns_empty_list_for_valid_address(self, client: TestClient) -> None:
        import rest_api

        with patch.object(
            rest_api.tx_analyser,
            "get_utxo",
            return_value={"utxo": []},
        ):
            response = client.get("/utxo/get", params={"address": TESTNET_ADDRESS})
        assert response.status_code == 200
        assert response.json() == {"utxo": []}

    def test_balance_returns_zero_for_valid_address(self, client: TestClient) -> None:
        import rest_api

        with patch.object(rest_api.block_manager, "get_block_height", return_value=0), patch.object(
            rest_api.tx_analyser,
            "get_balance",
            return_value={"confirmed": 0, "unconfirmed": 0},
        ):
            response = client.get("/utxo/balance", params={"address": TESTNET_ADDRESS})
        assert response.status_code == 200
        assert response.json() == {"confirmed": 0, "unconfirmed": 0}

    def test_broadcast_tx_hex_returns_503_when_rust_unreachable(
        self,
        client: TestClient,
    ) -> None:
        import rest_api
        import requests

        mock_tx = MagicMock()
        mock_tx.hash = VALID_HASH
        with patch.object(rest_api, "CTransaction", return_value=mock_tx), patch.object(
            rest_api.tx_analyser,
            "tx_exist",
            return_value=False,
        ), patch.object(
            rest_api.requests,
            "post",
            side_effect=requests.exceptions.ConnectionError("connection refused"),
        ):
            response = client.post("/tx/hex", json={"tx": "0100000001"})
        assert response.status_code == 503
        assert "Unable to connect with Rust service" in response.json()["failure"]

    def test_add_monitor_rejects_duplicate_name(self, client: TestClient) -> None:
        import rest_api

        monitor = {
            "name": "CoCv1",
            "track_descendants": False,
            "address": TESTNET_ADDRESS,
            "locking_script_pattern": None,
        }
        with patch.object(rest_api.collection, "is_valid_collection", return_value=True):
            response = client.post("/collection/monitor", json=monitor)
        assert response.status_code == 422
        assert "already exists" in response.json()["failed"]

    def test_delete_monitor_rejects_unknown_name(self, client: TestClient) -> None:
        import rest_api

        with patch.object(rest_api.collection, "is_valid_collection", return_value=False):
            response = client.delete(
                "/collection/monitor",
                params={"monitor_name": "missing-monitor"},
            )
        assert response.status_code == 422
        assert "does not exist" in response.json()["failed"]


    def test_delete_monitor_rejects_static_collection(self, client: TestClient) -> None:
        import rest_api

        with patch.object(rest_api.collection, "is_valid_collection", return_value=True), patch.object(
            rest_api.collection,
            "is_valid_dynamic_collection",
            return_value=False,
        ):
            response = client.delete(
                "/collection/monitor",
                params={"monitor_name": "CoCv1"},
            )
        assert response.status_code == 422
        assert "dynamic monitor" in response.json()["failed"]

    def test_bcast03_rejects_duplicate_transaction(self, client: TestClient) -> None:
        import rest_api

        mock_tx = MagicMock()
        mock_tx.hash = VALID_HASH
        with patch.object(rest_api, "CTransaction", return_value=mock_tx), patch.object(
            rest_api.tx_analyser,
            "tx_exist",
            return_value=True,
        ):
            response = client.post("/tx/hex", json={"tx": MINIMAL_TX_HEX})
        assert response.status_code == 422
        assert "already exists" in response.json()["failure"]

    def test_bcast04_proxies_valid_transaction_to_rust(self, client: TestClient) -> None:
        import rest_api

        mock_tx = MagicMock()
        mock_tx.hash = VALID_HASH
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_response.json.return_value = {"status": "Success", "detail": VALID_HASH}
        with patch.object(rest_api, "CTransaction", return_value=mock_tx), patch.object(
            rest_api.tx_analyser,
            "tx_exist",
            return_value=False,
        ), patch.object(rest_api.requests, "post", return_value=mock_response) as post:
            response = client.post("/tx/hex", json={"tx": MINIMAL_TX_HEX})
        assert response.status_code == 200
        assert response.json()["status"] == "Success"
        post.assert_called_once()
        assert post.call_args.args[0].endswith("/tx/raw")

    def test_mon01_adds_dynamic_monitor_via_rust(self, client: TestClient) -> None:
        import rest_api

        monitor = {
            "name": "new-monitor",
            "track_descendants": False,
            "address": TESTNET_ADDRESS,
            "locking_script_pattern": None,
        }
        mock_response = MagicMock()
        mock_response.status_code = 200
        with patch.object(rest_api.collection, "is_valid_collection", return_value=False), patch.object(
            rest_api.collection,
            "add_monitor",
        ) as add_monitor, patch.object(
            rest_api.requests,
            "post",
            return_value=mock_response,
        ) as post:
            response = client.post("/collection/monitor", json=monitor)
        assert response.status_code == 200
        post.assert_called_once()
        assert post.call_args.args[0].endswith("/collection/monitor")
        add_monitor.assert_called_once()
