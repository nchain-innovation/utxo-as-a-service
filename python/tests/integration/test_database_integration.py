from database import database


class TestDatabaseIntegration:
    def test_select_one(self, configured_services) -> None:
        result = database.query("SELECT 1")
        assert result == [(1,)]
