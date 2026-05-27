from fastapi.testclient import TestClient

from helpers import insert_sample_block, insert_sample_utxo

SAMPLE_HASH = "a" * 64
SAMPLE_HEIGHT = 100
TESTNET_ADDRESS = "mgzhRq55hEYFgyCrtNxEsP1MdusZZ31hH5"
TESTNET_PUBKEYHASH = "10375cfe32b917cd24ca1038f824cd00f7391859"
SAMPLE_TX_HASH = "d" * 64


class TestRestApiIntegration:
    def test_health_with_database(self, client: TestClient) -> None:
        response = client.get("/health")
        assert response.status_code == 200
        body = response.json()
        assert body["status"] == "ok"
        assert body["service"] == "uaas-web"
        assert "database" not in body

    def test_status_with_empty_blocks(self, client: TestClient, clean_blocks) -> None:
        response = client.get("/status")
        assert response.status_code == 200
        body = response.json()
        assert body["network"] == "testnet"
        assert body["block height"] == 0
        assert body["last block time"] == "unknown"
        assert body["number of txs"] == 0
        assert body["number of utxo entries"] == 0
        assert body["number of mempool entries"] == 0

    def test_status_returns_database_counts(
        self,
        client: TestClient,
        mysql_url: str,
        clean_blocks,
    ) -> None:
        insert_sample_block(mysql_url, SAMPLE_HEIGHT, SAMPLE_HASH)

        response = client.get("/status")
        assert response.status_code == 200
        body = response.json()
        assert body["network"] == "testnet"
        assert body["block height"] == SAMPLE_HEIGHT
        assert body["number of txs"] == 0
        assert body["number of utxo entries"] == 0
        assert body["number of mempool entries"] == 0

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

    def test_block_not_found_at_height(self, client: TestClient, clean_blocks) -> None:
        response = client.get("/block/height", params={"height": 999})
        assert response.status_code == 200
        assert response.json() == {"block": "block height 999 not found"}

    def test_block_not_found_at_hash(self, client: TestClient, clean_blocks) -> None:
        missing_hash = "f" * 64
        response = client.get("/block/hash", params={"hash": missing_hash})
        assert response.status_code == 200
        assert response.json() == {"block": f"block hash {missing_hash} not found"}

    def test_block_latest_and_last(
        self,
        client: TestClient,
        mysql_url: str,
        clean_blocks,
    ) -> None:
        insert_sample_block(mysql_url, SAMPLE_HEIGHT, SAMPLE_HASH)

        latest = client.get("/block/latest")
        assert latest.status_code == 200
        blocks = latest.json()["blocks"]
        assert len(blocks) == 1
        assert blocks[0]["height"] == SAMPLE_HEIGHT

        last = client.get("/block/last")
        assert last.status_code == 200
        assert last.json()["height"] == SAMPLE_HEIGHT

        last_hex = client.get("/block/last/hex")
        assert last_hex.status_code == 200
        assert "block" in last_hex.json()

    def test_block_last_returns_503_when_empty(self, client: TestClient, clean_blocks) -> None:
        response = client.get("/block/last")
        assert response.status_code == 503
        assert response.json() == {}

    def test_utxo_empty_for_address(
        self,
        client: TestClient,
        clean_utxo,
    ) -> None:
        response = client.get("/utxo/get", params={"address": TESTNET_ADDRESS})
        assert response.status_code == 200
        assert response.json() == {"utxo": []}

    def test_balance_splits_confirmed_and_unconfirmed(
        self,
        client: TestClient,
        mysql_url: str,
        clean_blocks,
        clean_utxo,
    ) -> None:
        insert_sample_block(mysql_url, SAMPLE_HEIGHT, SAMPLE_HASH)
        insert_sample_utxo(
            mysql_url,
            SAMPLE_TX_HASH,
            TESTNET_PUBKEYHASH,
            height=90,
            satoshis=100,
        )
        insert_sample_utxo(
            mysql_url,
            "e" * 64,
            TESTNET_PUBKEYHASH,
            height=99,
            satoshis=50,
        )

        response = client.get("/utxo/balance", params={"address": TESTNET_ADDRESS})
        assert response.status_code == 200
        body = response.json()
        assert body["confirmed"] == 100
        assert body["unconfirmed"] == 50

    def test_get_collections(self, client: TestClient) -> None:
        response = client.get("/collection")
        assert response.status_code == 200
        assert response.json()["collections"] == []
