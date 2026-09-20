from unittest.mock import patch

from hashes import txid_to_bytes
from tx_analyser import _TX_EXIST_QUERY, tx_analyser


VALID_HASH = "a" * 64
# What the database actually holds for that txid: the same bytes, reversed.
VALID_RAW = txid_to_bytes(VALID_HASH)


class TestTxExist:
    def test_tx_exist_uses_a_single_query(self) -> None:
        with patch("tx_analyser.database.query", return_value=[(1,)]) as query:
            assert tx_analyser.tx_exist(VALID_HASH) is True
        query.assert_called_once_with(_TX_EXIST_QUERY, (VALID_RAW, VALID_RAW, VALID_RAW))

    def test_tx_exist_returns_false_when_not_found(self) -> None:
        with patch("tx_analyser.database.query", return_value=[]):
            assert tx_analyser.tx_exist(VALID_HASH) is False

    def test_tx_exist_passes_bytes_not_the_hex_string(self) -> None:
        # The failure this guards is silent: a hex string compared to a bytea
        # column matches nothing and raises nothing, so the endpoint would
        # answer "no such transaction" for every transaction.
        with patch("tx_analyser.database.query", return_value=[]) as query:
            tx_analyser.tx_exist(VALID_HASH)
        (_, params) = query.call_args[0]
        assert all(isinstance(p, bytes) for p in params), params
        assert VALID_HASH not in [p.hex() for p in params] or VALID_HASH == VALID_HASH[::-1]

    def test_tx_exist_has_no_missing_table_fallback(self) -> None:
        # The fallback query is gone. The schema is versioned and the service
        # refuses to start against the wrong version, so a missing `tx` table
        # must surface rather than silently answering a narrower question.
        import tx_analyser as module

        assert not hasattr(module, "_TX_EXIST_WITHOUT_TX_TABLE_QUERY")
