from unittest.mock import patch

from mysql.connector.errors import ProgrammingError

from tx_analyser import (
    _TX_EXIST_QUERY,
    _TX_EXIST_WITHOUT_TX_TABLE_QUERY,
    tx_analyser,
)


VALID_HASH = "a" * 64


class TestTxExist:
    def test_tx_exist_uses_single_query_when_tx_table_available(self) -> None:
        with patch("tx_analyser.database.query", return_value=[(1,)]) as query:
            assert tx_analyser.tx_exist(VALID_HASH) is True
        query.assert_called_once_with(_TX_EXIST_QUERY, (VALID_HASH, VALID_HASH, VALID_HASH))

    def test_tx_exist_returns_false_when_not_found(self) -> None:
        with patch("tx_analyser.database.query", return_value=[]):
            assert tx_analyser.tx_exist(VALID_HASH) is False

    def test_tx_exist_falls_back_when_tx_table_missing(self) -> None:
        with patch("tx_analyser.database.query") as query:
            query.side_effect = [
                ProgrammingError("Table 'main_uaas_db.tx' doesn't exist"),
                [(1,)],
            ]
            assert tx_analyser.tx_exist(VALID_HASH) is True
        assert query.call_count == 2
        query.assert_any_call(_TX_EXIST_QUERY, (VALID_HASH, VALID_HASH, VALID_HASH))
        query.assert_any_call(
            _TX_EXIST_WITHOUT_TX_TABLE_QUERY,
            (VALID_HASH, VALID_HASH),
        )
