from helpers import fetch


class TestMempoolRequirements:
    def test_sync04_mempool_table_accepts_transaction_row(self, postgres_url: str) -> None:
        from hashes import txid_to_bytes

        tx_hash = "1" * 64
        raw = txid_to_bytes(tx_hash)
        try:
            # `time` became `seen_at`, an int unix timestamp became a
            # timestamptz, and the tx column is bytea rather than hex text.
            fetch(
                postgres_url,
                "INSERT INTO mempool (hash, locktime, fee, seen_at, tx) "
                "VALUES (%s, %s, %s, to_timestamp(%s), %s)",
                (raw, 0, 500, 1_700_000_000, bytes.fromhex("0100000001")),
            )
            rows = fetch(
                postgres_url,
                "SELECT fee, extract(epoch FROM seen_at)::bigint FROM mempool WHERE hash = %s",
                (raw,),
            )
            fee, seen_at_epoch = rows[0]
            assert fee == 500
            assert seen_at_epoch == 1_700_000_000
        finally:
            fetch(postgres_url, "DELETE FROM mempool WHERE hash = %s", (raw,))
