from tx_analyser import tx_analyser

from helpers import insert_sample_utxo

TESTNET_PUBKEYHASH = "10375cfe32b917cd24ca1038f824cd00f7391859"
SAMPLE_TX_HASH = "d" * 64


class TestTxAnalyserIntegration:
    def test_get_utxo_returns_matching_rows(
        self,
        configured_services,
        postgres_url: str,
        clean_utxo,
    ) -> None:
        insert_sample_utxo(
            postgres_url,
            SAMPLE_TX_HASH,
            TESTNET_PUBKEYHASH,
            height=10,
            satoshis=250,
        )

        result = tx_analyser.get_utxo(TESTNET_PUBKEYHASH)
        assert result == {
            "utxo": [
                {
                    "height": 10,
                    "tx_pos": 0,
                    "tx_hash": SAMPLE_TX_HASH,
                    "value": 250,
                }
            ]
        }

    def test_get_balance_sums_satoshi_by_confirmation(
        self,
        configured_services,
        postgres_url: str,
        clean_utxo,
    ) -> None:
        insert_sample_utxo(
            postgres_url,
            SAMPLE_TX_HASH,
            TESTNET_PUBKEYHASH,
            height=1,
            satoshis=100,
        )
        insert_sample_utxo(
            postgres_url,
            "e" * 64,
            TESTNET_PUBKEYHASH,
            height=-1,
            satoshis=25,
        )

        result = tx_analyser.get_balance(TESTNET_PUBKEYHASH, blockheight=10)
        assert result["confirmed"] == 100
        assert result["unconfirmed"] == 25


class TestHashInterop:
    """The txid representation, end to end through a real database.

    The unit tests pin Python's conversion against a literal that
    rust/tests/hash_order.rs asserts chain_gang produces. This closes the loop:
    the bytes Python actually stores are those bytes, and what comes back out
    is the txid the API started with.
    """

    # Same pair as python/tests/test_hashes.py and rust/tests/hash_order.rs.
    DISPLAY = "00000000000000000545267003727771023c9822756f187cbee83a5329ffecd8"
    STORED_HEX = "d8ecff29533ae8be7c186f7522983c0271777203702645050000000000000000"
    IDENTIFIER = "7c78584493557fac782023a4ad591b64545929d9"

    def test_hashio01_python_stores_the_bytes_rust_would_write(
        self,
        configured_services,
        postgres_url: str,
        clean_utxo,
    ) -> None:
        from helpers import fetch, insert_sample_utxo

        insert_sample_utxo(
            postgres_url,
            tx_hash=self.DISPLAY,
            pubkeyhash=self.IDENTIFIER,
            height=100,
            satoshis=1234,
        )
        rows = fetch(postgres_url, "SELECT txid, identifier FROM utxo")
        assert len(rows) == 1
        stored_txid, stored_identifier = rows[0]
        # The txid is reversed on the way in; the identifier is not.
        assert bytes(stored_txid).hex() == self.STORED_HEX
        assert bytes(stored_identifier).hex() == self.IDENTIFIER

    def test_hashio02_the_api_returns_the_txid_it_was_given(
        self,
        configured_services,
        postgres_url: str,
        clean_utxo,
    ) -> None:
        from helpers import insert_sample_utxo
        from tx_analyser import tx_analyser

        insert_sample_utxo(
            postgres_url,
            tx_hash=self.DISPLAY,
            pubkeyhash=self.IDENTIFIER,
            height=100,
            satoshis=1234,
        )
        result = tx_analyser.get_utxo(self.IDENTIFIER)
        assert len(result["utxo"]) == 1
        # Reversed in, reversed out. A conversion dropped at either end would
        # return STORED_HEX here and nothing would raise.
        assert result["utxo"][0]["tx_hash"] == self.DISPLAY
        assert result["utxo"][0]["value"] == 1234
