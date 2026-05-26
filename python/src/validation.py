import re

_HASH_PATTERN = re.compile(r"^[0-9a-fA-F]{64}$")
_MONITOR_NAME_PATTERN = re.compile(r"^[a-zA-Z0-9_-]{1,64}$")
_HEX_PATTERN = re.compile(r"^[0-9a-fA-F]+$")

MAX_BLOCK_HEIGHT = 100_000_000


def validate_tx_hash(value: str) -> str:
    if not _HASH_PATTERN.match(value):
        raise ValueError(
            "Invalid transaction hash: expected 64 hexadecimal characters"
        )
    return value.lower()


def validate_block_hash(value: str) -> str:
    if not _HASH_PATTERN.match(value):
        raise ValueError(
            "Invalid block hash: expected 64 hexadecimal characters"
        )
    return value.lower()


def validate_block_height(value: int) -> int:
    if value < 0:
        raise ValueError("Invalid block height: must be non-negative")
    if value > MAX_BLOCK_HEIGHT:
        raise ValueError(
            f"Invalid block height: must not exceed {MAX_BLOCK_HEIGHT}"
        )
    return value


def validate_monitor_name(value: str) -> str:
    if not _MONITOR_NAME_PATTERN.match(value):
        raise ValueError(
            "Invalid monitor name: must be 1-64 alphanumeric, hyphen, "
            "or underscore characters"
        )
    return value


def validate_hex_string(value: str) -> str:
    if not value or len(value) % 2 != 0:
        raise ValueError(
            "Invalid hex string: must be non-empty with even length"
        )
    if not _HEX_PATTERN.match(value):
        raise ValueError(
            "Invalid hex string: contains non-hexadecimal characters"
        )
    return value.lower()


def validate_locking_script_pattern(value: str) -> str:
    if not value or len(value) > 512:
        raise ValueError(
            "Invalid locking script pattern: length must be 1-512 characters"
        )
    return value
