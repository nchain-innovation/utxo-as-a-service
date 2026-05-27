from tx_analyser import tx_analyser

from helpers import insert_sample_utxo

TESTNET_PUBKEYHASH = "10375cfe32b917cd24ca1038f824cd00f7391859"
SAMPLE_TX_HASH = "d" * 64


class TestTxAnalyserIntegration:
    def test_get_utxo_returns_matching_rows(
        self,
        configured_services,
        mysql_url: str,
        clean_utxo,
    ) -> None:
        insert_sample_utxo(
            mysql_url,
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
        mysql_url: str,
        clean_utxo,
    ) -> None:
        insert_sample_utxo(
            mysql_url,
            SAMPLE_TX_HASH,
            TESTNET_PUBKEYHASH,
            height=1,
            satoshis=100,
        )
        insert_sample_utxo(
            mysql_url,
            "e" * 64,
            TESTNET_PUBKEYHASH,
            height=-1,
            satoshis=25,
        )

        result = tx_analyser.get_balance(TESTNET_PUBKEYHASH, blockheight=10)
        assert result["confirmed"] == 100
        assert result["unconfirmed"] == 25
