import os
import textwrap

import pytest
from fastapi.testclient import TestClient

from helpers import insert_sample_block


VALID_HASH = "a" * 64
TESTNET_ADDRESS = "mgzhRq55hEYFgyCrtNxEsP1MdusZZ31hH5"
MINIMAL_TX_HEX = "0100000001"


class TestSystemRequirementsIntegration:
    def test_ops06_core_tables_exist(self, mysql_url: str) -> None:
        import mysql.connector

        from helpers import parse_mysql_url

        db = parse_mysql_url(mysql_url)
        connection = mysql.connector.connect(
            host=db["host"],
            port=db["port"],
            user=db["user"],
            password=db["password"],
            database=db["database"],
        )
        cursor = connection.cursor()
        try:
            cursor.execute("SHOW TABLES")
            tables = {row[0] for row in cursor.fetchall()}
            for table in ("blocks", "tx", "utxo", "mempool"):
                assert table in tables
        finally:
            cursor.close()
            connection.close()

    def test_data03_mempool_table_has_fee_and_time_columns(self, mysql_url: str) -> None:
        import mysql.connector

        from helpers import parse_mysql_url

        db = parse_mysql_url(mysql_url)
        connection = mysql.connector.connect(
            host=db["host"],
            port=db["port"],
            user=db["user"],
            password=db["password"],
            database=db["database"],
        )
        cursor = connection.cursor()
        try:
            cursor.execute("SHOW COLUMNS FROM mempool")
            columns = {row[0] for row in cursor.fetchall()}
            assert {"fee", "time", "locktime", "tx"}.issubset(columns)
        finally:
            cursor.close()
            connection.close()

    def test_sync09_connect_table_accepts_events(self, mysql_url: str) -> None:
        import mysql.connector

        from helpers import parse_mysql_url

        db = parse_mysql_url(mysql_url)
        connection = mysql.connector.connect(
            host=db["host"],
            port=db["port"],
            user=db["user"],
            password=db["password"],
            database=db["database"],
        )
        cursor = connection.cursor()
        try:
            cursor.execute(
                """
                CREATE TABLE IF NOT EXISTS connect (
                    date VARCHAR(64),
                    ip VARCHAR(64),
                    event VARCHAR(64)
                )
                """
            )
            cursor.execute(
                "INSERT INTO connect (date, ip, event) VALUES (%s, %s, %s)",
                ("2026-01-01", "127.0.0.1", "Connect"),
            )
            connection.commit()
            cursor.execute("SELECT event FROM connect WHERE ip = %s", ("127.0.0.1",))
            assert cursor.fetchone()[0] == "Connect"
        finally:
            cursor.execute("DELETE FROM connect WHERE ip = %s", ("127.0.0.1",))
            connection.commit()
            cursor.close()
            connection.close()

    def test_api09_tx_proof_returns_merkle_branches(
        self,
        client: TestClient,
        mysql_url: str,
        clean_blocks,
    ) -> None:
        import mysql.connector

        from helpers import parse_mysql_url

        block_hash = "d" * 64
        insert_sample_block(mysql_url, height=10, block_hash=block_hash)
        db = parse_mysql_url(mysql_url)
        connection = mysql.connector.connect(
            host=db["host"],
            port=db["port"],
            user=db["user"],
            password=db["password"],
            database=db["database"],
        )
        cursor = connection.cursor()
        try:
            cursor.execute(
                """
                UPDATE blocks SET merkle_root = %s WHERE hash = %s
                """,
                ("c" * 64, block_hash),
            )
            cursor.execute(
                """
                INSERT INTO tx (hash, height, blockindex, txsize, satoshis)
                VALUES (%s, %s, %s, %s, %s)
                """,
                (VALID_HASH, 10, 0, 250, 1000),
            )
            connection.commit()
        finally:
            cursor.close()
            connection.close()

        response = client.get("/tx/proof", params={"hash": VALID_HASH})
        assert response.status_code == 200
        body = response.json()
        assert body["tx_hash"] == VALID_HASH
        assert body["block_hash"] == block_hash
        assert isinstance(body["branches"], list)

    def test_data01_duplicate_block_hash_rejected(self, mysql_url: str) -> None:
        import mysql.connector

        from helpers import parse_mysql_url

        block_hash = "f" * 64
        insert_sample_block(mysql_url, height=20, block_hash=block_hash)
        db = parse_mysql_url(mysql_url)
        connection = mysql.connector.connect(
            host=db["host"],
            port=db["port"],
            user=db["user"],
            password=db["password"],
            database=db["database"],
        )
        cursor = connection.cursor()
        try:
            with pytest.raises(mysql.connector.errors.IntegrityError):
                insert_sample_block(mysql_url, height=21, block_hash=block_hash)
        finally:
            cursor.execute("DELETE FROM blocks WHERE hash = %s", (block_hash,))
            connection.commit()
            cursor.close()
            connection.close()