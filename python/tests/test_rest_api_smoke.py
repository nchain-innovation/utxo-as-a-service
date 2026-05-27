from unittest.mock import patch

import pytest
from fastapi.testclient import TestClient


@pytest.fixture(scope="module")
def client() -> TestClient:
    import rest_api

    return TestClient(rest_api.app)


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
        assert response.json()["status"] == "ok"

    def test_invalid_tx_hash_returns_422(self, client: TestClient) -> None:
        response = client.get("/tx", params={"hash": "not-a-hash"})
        assert response.status_code == 422
        assert "failure" in response.json()

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
