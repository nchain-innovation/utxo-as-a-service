from database import database


class TestDatabaseIntegration:
    def test_select_one(self, configured_services) -> None:
        result = database.query("SELECT 1")
        assert result == [(1,)]

    def test_parameterized_query(self, configured_services) -> None:
        result = database.query("SELECT %s AS value", (42,))
        assert result == [(42,)]

    def test_blocks_table_exists(self, configured_services) -> None:
        result = database.query(
            "SELECT COUNT(*) FROM information_schema.tables "
            # DATABASE() is MySQL's. current_schema() is the equivalent, and
            # is what everything else in this suite uses.
            "WHERE table_schema = current_schema() AND table_name = 'blocks'"
        )
        assert result[0][0] == 1
