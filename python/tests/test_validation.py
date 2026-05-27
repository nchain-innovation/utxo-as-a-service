import pytest

from validation import (
    MAX_BLOCK_HEIGHT,
    validate_block_hash,
    validate_block_height,
    validate_hex_string,
    validate_locking_script_pattern,
    validate_monitor_name,
    validate_tx_hash,
)

VALID_HASH = "a" * 64


class TestValidateTxHash:
    def test_accepts_valid_hash(self) -> None:
        assert validate_tx_hash(VALID_HASH.upper()) == VALID_HASH

    def test_rejects_short_hash(self) -> None:
        with pytest.raises(ValueError, match="64 hexadecimal"):
            validate_tx_hash("abc")

    def test_rejects_non_hex(self) -> None:
        with pytest.raises(ValueError, match="64 hexadecimal"):
            validate_tx_hash("g" * 64)


class TestValidateBlockHash:
    def test_accepts_valid_hash(self) -> None:
        assert validate_block_hash(VALID_HASH) == VALID_HASH


class TestValidateBlockHeight:
    def test_accepts_zero(self) -> None:
        assert validate_block_height(0) == 0

    def test_accepts_max_height(self) -> None:
        assert validate_block_height(MAX_BLOCK_HEIGHT) == MAX_BLOCK_HEIGHT

    def test_rejects_negative(self) -> None:
        with pytest.raises(ValueError, match="non-negative"):
            validate_block_height(-1)

    def test_rejects_above_max(self) -> None:
        with pytest.raises(ValueError, match=str(MAX_BLOCK_HEIGHT)):
            validate_block_height(MAX_BLOCK_HEIGHT + 1)


class TestValidateMonitorName:
    def test_accepts_valid_name(self) -> None:
        assert validate_monitor_name("CoCv1") == "CoCv1"

    def test_rejects_empty(self) -> None:
        with pytest.raises(ValueError, match="monitor name"):
            validate_monitor_name("")

    def test_rejects_invalid_characters(self) -> None:
        with pytest.raises(ValueError, match="monitor name"):
            validate_monitor_name("bad name")


class TestValidateHexString:
    def test_accepts_valid_hex(self) -> None:
        assert validate_hex_string("DEADBEEF") == "deadbeef"

    def test_rejects_empty(self) -> None:
        with pytest.raises(ValueError, match="even length"):
            validate_hex_string("")

    def test_rejects_odd_length(self) -> None:
        with pytest.raises(ValueError, match="even length"):
            validate_hex_string("abc")

    def test_rejects_non_hex(self) -> None:
        with pytest.raises(ValueError, match="non-hexadecimal"):
            validate_hex_string("zz")


class TestValidateLockingScriptPattern:
    def test_accepts_valid_pattern(self) -> None:
        pattern = "76a914[0-9a-f]{40}88ac"
        assert validate_locking_script_pattern(pattern) == pattern

    def test_rejects_empty(self) -> None:
        with pytest.raises(ValueError, match="1-512"):
            validate_locking_script_pattern("")

    def test_rejects_too_long(self) -> None:
        with pytest.raises(ValueError, match="1-512"):
            validate_locking_script_pattern("x" * 513)
