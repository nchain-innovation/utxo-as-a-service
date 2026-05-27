from fastapi.testclient import TestClient

from helpers import insert_sample_block

SAMPLE_HASH = "a" * 64
SAMPLE_HEIGHT = 100


class TestRestApiIntegration:
    def test_health_with_database(self, client: TestClient) -> None:
        response = client.get("/health")
        assert response.status_code == 200
        body = response.json()
        assert body["status"] == "ok"
        assert body["service"] == "uaas-web"

    def test_status_returns_database_counts(self, client: TestClient) -> None:
        response = client.get("/status")
        assert response.status_code == 200
        body = response.json()
        assert body["network"] == "testnet"
        assert "block height" in body
        assert "number of txs" in body
        assert "number of utxo entries" in body
        assert "number of mempool entries" in body

    def test_block_height_roundtrip(
        self,
        client: TestClient,
        mysql_url: str,
        clean_blocks,
    ) -> None:
        insert_sample_block(mysql_url, SAMPLE_HEIGHT, SAMPLE_HASH)

        response = client.get("/block/height", params={"height": SAMPLE_HEIGHT})
        assert response.status_code == 200
        body = response.json()
        assert body["height"] == SAMPLE_HEIGHT
        assert body["header"]["hash"] == SAMPLE_HASH

    def test_block_hash_lookup(
        self,
        client: TestClient,
        mysql_url: str,
        clean_blocks,
    ) -> None:
        insert_sample_block(mysql_url, SAMPLE_HEIGHT, SAMPLE_HASH)

        response = client.get("/block/hash", params={"hash": SAMPLE_HASH})
        assert response.status_code == 200
        body = response.json()
        assert body["block"]["height"] == SAMPLE_HEIGHT
        assert body["block"]["header"]["hash"] == SAMPLE_HASH

    def test_get_collections(self, client: TestClient) -> None:
        response = client.get("/collection")
        assert response.status_code == 200
        assert response.json()["collections"] == []
