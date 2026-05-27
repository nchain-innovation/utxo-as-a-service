class TestMempoolRequirements:
    def test_sync04_mempool_table_accepts_transaction_row(self, mysql_url: str) -> None:
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
        tx_hash = "1" * 64
        try:
            cursor.execute(
                """
                INSERT INTO mempool (hash, locktime, fee, time, tx)
                VALUES (%s, %s, %s, %s, %s)
                """,
                (tx_hash, 0, 500, 1_700_000_000, "0100000001"),
            )
            connection.commit()
            cursor.execute("SELECT fee, time FROM mempool WHERE hash = %s", (tx_hash,))
            fee, added_time = cursor.fetchone()
            assert fee == 500
            assert added_time == 1_700_000_000
        finally:
            cursor.execute("DELETE FROM mempool WHERE hash = %s", (tx_hash,))
            connection.commit()
            cursor.close()
            connection.close()
