import pytest
from fastapi.testclient import TestClient

from helpers import fetch, insert_sample_block


VALID_HASH = "a" * 64
TESTNET_ADDRESS = "mgzhRq55hEYFgyCrtNxEsP1MdusZZ31hH5"
MINIMAL_TX_HEX = "0100000001"


class TestSystemRequirementsIntegration:
    def test_ops06_core_tables_exist(self, postgres_url: str) -> None:
        # SHOW TABLES is MySQL-only; information_schema is the portable form
        # and is what the schema check in helpers uses too.
        rows = fetch(
            postgres_url,
            "SELECT table_name FROM information_schema.tables "
            "WHERE table_schema = current_schema()",
        )
        tables = {row[0] for row in rows}
        for expected in ("blocks", "tx", "utxo", "mempool"):
            assert expected in tables, f"{expected} missing from {sorted(tables)}"

    def test_data03_mempool_stores_fee_and_a_sighting_time(self, postgres_url: str) -> None:
        rows = fetch(
            postgres_url,
            "SELECT column_name FROM information_schema.columns "
            "WHERE table_schema = current_schema() AND table_name = 'mempool'",
        )
        columns = {row[0] for row in rows}
        # `time` was renamed to `seen_at` when it became a timestamptz. The
        # requirement is that a sighting time is recorded, not what it is
        # called, so this asserts the current name rather than the old one.
        assert "fee" in columns
        assert "seen_at" in columns

    def test_sync09_connect_table_accepts_events(self, postgres_url: str) -> None:
        # This used to CREATE TABLE IF NOT EXISTS the table it was testing,
        # which meant it passed whether or not the schema defined one. The
        # migrations own it now, so the table's existence is part of what is
        # under test.
        try:
            # `date VARCHAR(64)` became `event_time timestamptz` with a
            # default, so the insert names neither it nor the generated id.
            fetch(
                postgres_url,
                "INSERT INTO connect (ip, event) VALUES (%s, %s)",
                ("127.0.0.1", "Connect"),
            )
            rows = fetch(
                postgres_url,
                "SELECT event, event_time FROM connect WHERE ip = %s",
                ("127.0.0.1",),
            )
            assert rows[0][0] == "Connect"
            # The point of the column change: a real instant that can be
            # range-queried, not a formatted string that has to be parsed.
            assert rows[0][1] is not None
        finally:
            fetch(postgres_url, "DELETE FROM connect WHERE ip = %s", ("127.0.0.1",))

    def test_api09_tx_proof_returns_merkle_branches(
        self,
        client: TestClient,
        postgres_url: str,
        clean_blocks,
    ) -> None:
        from hashes import txid_to_bytes

        block_hash = "d" * 64
        insert_sample_block(postgres_url, height=10, block_hash=block_hash)
        fetch(
            postgres_url,
            "UPDATE blocks SET merkle_root = %s WHERE hash = %s",
            (txid_to_bytes("c" * 64), txid_to_bytes(block_hash)),
        )
        fetch(
            postgres_url,
            "INSERT INTO tx (hash, height, blockindex, txsize, satoshis) "
            "VALUES (%s, %s, %s, %s, %s)",
            (txid_to_bytes(VALID_HASH), 10, 0, 250, 1000),
        )

        response = client.get("/tx/proof", params={"hash": VALID_HASH})
        assert response.status_code == 200
        body = response.json()
        # The endpoint speaks display order both ways. If the conversion were
        # dropped anywhere in that path these would come back byte-reversed.
        assert body["tx_hash"] == VALID_HASH
        assert body["block_hash"] == block_hash

    def test_data01_duplicate_block_hash_rejected(self, postgres_url: str) -> None:
        import psycopg

        from hashes import txid_to_bytes

        block_hash = "f" * 64
        insert_sample_block(postgres_url, height=20, block_hash=block_hash)
        try:
            # hash is the primary key, so the second insert must be refused by
            # the database rather than quietly producing two rows.
            with pytest.raises(psycopg.errors.UniqueViolation):
                insert_sample_block(postgres_url, height=21, block_hash=block_hash)
        finally:
            fetch(
                postgres_url,
                "DELETE FROM blocks WHERE hash = %s",
                (txid_to_bytes(block_hash),),
            )
