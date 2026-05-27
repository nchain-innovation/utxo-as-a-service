from database import _requires_commit


class TestRequiresCommit:
    def test_select_does_not_require_commit(self) -> None:
        assert _requires_commit("SELECT 1") is False
        assert _requires_commit("  select * from utxo") is False

    def test_write_statements_require_commit(self) -> None:
        assert _requires_commit("INSERT INTO tx VALUES (%s)") is True
        assert _requires_commit("UPDATE utxo SET satoshis = 1") is True
        assert _requires_commit("DELETE FROM mempool WHERE hash = %s") is True
        assert _requires_commit("REPLACE INTO utxo VALUES (%s, %s)") is True
