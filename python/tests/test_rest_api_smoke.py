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
