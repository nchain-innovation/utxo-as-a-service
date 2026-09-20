"""Converting between the hashes the API speaks and the bytes the database holds.

There are two independent changes to get right here, and getting either wrong
fails silently — comparing the wrong bytes to a `bytea` column returns no rows
and raises nothing.

**Representation.** Hashes were 64-character hex in a `varchar(64)`. They are
now 32 raw bytes in a `bytea`.

**Byte order.** This is the one that is easy to miss. The old Rust code wrote
`Hash256::encode()`, which *reverses* before hex-encoding — the conventional
display order for a Bitcoin txid, and the same order the REST API speaks. The
new code binds `Hash256.0` directly, which is internal order. So the database
now holds the reverse of what it used to, and a caller's hex string has to be
reversed on the way in and on the way out.

Identifiers are different and must not be reversed. An identifier is the bytes
a locking-script pattern captured, taken from the script as it appears, so
script order is its natural order — exactly what the old `script_to_pubkeyhash`
returned the hex of.
"""

TXID_LEN = 32


class HashFormatError(ValueError):
    """Raised when a hash string is not hex, or is the wrong length."""


def _decode(value: str, expected_len: int, what: str) -> bytes:
    try:
        raw = bytes.fromhex(value)
    except ValueError as err:
        raise HashFormatError(f"{what} is not valid hex: {value!r}") from err
    if len(raw) != expected_len:
        raise HashFormatError(
            f"{what} is {len(raw)} bytes, expected {expected_len}: {value!r}"
        )
    return raw


def txid_to_bytes(display_hex: str) -> bytes:
    """A txid as the API states it, to the bytes the database stores.

    Reverses: the API speaks display order, the database holds internal order.
    """
    return _decode(display_hex, TXID_LEN, "txid")[::-1]


def txid_from_bytes(stored: bytes) -> str:
    """The bytes the database stores, back to the txid the API states."""
    raw = bytes(stored)
    if len(raw) != TXID_LEN:
        raise HashFormatError(f"stored txid is {len(raw)} bytes, expected {TXID_LEN}")
    return raw[::-1].hex()


def identifier_to_bytes(hex_str: str) -> bytes:
    """An identifier as the API states it, to the bytes the database stores.

    Not reversed. See the module docstring.
    """
    try:
        return bytes.fromhex(hex_str)
    except ValueError as err:
        raise HashFormatError(f"identifier is not valid hex: {hex_str!r}") from err


def identifier_from_bytes(stored: bytes | None) -> str | None:
    """The bytes the database stores, back to the identifier the API states.

    `None` passes through: the column is nullable, because a pattern need not
    declare an identifier group.
    """
    if stored is None:
        return None
    return bytes(stored).hex()
