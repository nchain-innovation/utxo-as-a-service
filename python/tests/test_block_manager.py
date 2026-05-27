from unittest.mock import patch

from block_manager import BlockManager


class TestBlockManager:
    def test_get_block_height_returns_zero_when_table_empty(self) -> None:
        manager = BlockManager()
        with patch(
            "block_manager.database.query",
            return_value=[(None,)],
        ):
            assert manager.get_block_height() == 0

    def test_get_block_height_returns_max_height(self) -> None:
        manager = BlockManager()
        with patch(
            "block_manager.database.query",
            return_value=[(42,)],
        ):
            assert manager.get_block_height() == 42

    def test_get_last_block_time_returns_unknown_when_empty(self) -> None:
        manager = BlockManager()
        with patch("block_manager.database.query", return_value=[]):
            assert manager.get_last_block_time() == "unknown"

    def test_get_last_block_time_formats_timestamp(self) -> None:
        manager = BlockManager()
        with patch(
            "block_manager.database.query",
            return_value=[(1_700_000_000,)],
        ):
            assert manager.get_last_block_time() == "2023-11-14 22:13:20"

    def test_get_block_at_height_not_found(self) -> None:
        manager = BlockManager()
        with patch("block_manager.database.query", return_value=[]):
            result = manager.get_block_at_height(999)
        assert result == {"block": "block height 999 not found"}

    def test_get_block_at_hash_not_found(self) -> None:
        manager = BlockManager()
        block_hash = "a" * 64
        with patch("block_manager.database.query", return_value=[]):
            result = manager.get_block_at_hash(block_hash)
        assert result == {"block": f"block hash {block_hash} not found"}
