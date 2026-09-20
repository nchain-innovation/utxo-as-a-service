"""The hash boundary between the API and the database.

Every assertion here exists because getting it wrong is silent: the wrong bytes
against a `bytea` column return no rows and raise nothing.
"""

import pytest

from hashes import (
    HashFormatError,
    identifier_from_bytes,
    identifier_to_bytes,
    txid_from_bytes,
    txid_to_bytes,
)


# A real testnet txid, as the API states it.
DISPLAY = "00000000000000000545267003727771023c9822756f187cbee83a5329ffecd8"
# The same txid as the database stores it. Not derived from DISPLAY here on
# purpose: this literal is the cross-language contract, and the Rust half
# asserts the identical pair in rust/tests/hash_order.rs. Deriving it would
# make this test agree with itself rather than with the other component.
STORED_HEX = "d8ecff29533ae8be7c186f7522983c0271777203702645050000000000000000"
INTERNAL = bytes.fromhex(STORED_HEX)


class TestTxidConversion:
    def test_hash01_display_hex_converts_to_internal_byte_order(self) -> None:
        # The conversion the database needs. Rust used to write
        # Hash256::encode(), which reverses; it now binds Hash256.0 directly,
        # so the stored bytes are the reverse of the displayed hex.
        assert txid_to_bytes(DISPLAY) == INTERNAL

    def test_hash02_stored_bytes_convert_back_to_display_hex(self) -> None:
        assert txid_from_bytes(INTERNAL) == DISPLAY

    def test_hash03_the_conversion_round_trips(self) -> None:
        assert txid_from_bytes(txid_to_bytes(DISPLAY)) == DISPLAY

    def test_hash04_the_reversal_is_not_a_no_op(self) -> None:
        # Guards the tests above against a conversion that forgot to reverse:
        # with a palindromic fixture every assertion here would still pass.
        assert txid_to_bytes(DISPLAY) != bytes.fromhex(DISPLAY)

    def test_hash05_a_non_hex_txid_is_rejected_not_silently_wrong(self) -> None:
        with pytest.raises(HashFormatError):
            txid_to_bytes("not hex at all" + "0" * 50)

    def test_hash06_a_short_txid_is_rejected(self) -> None:
        # bytes.fromhex accepts this happily; the length check is what stops it
        # reaching the database and matching nothing.
        with pytest.raises(HashFormatError):
            txid_to_bytes("aabbcc")

    def test_hash07_an_odd_length_txid_is_rejected(self) -> None:
        with pytest.raises(HashFormatError):
            txid_to_bytes("a" * 63)

    def test_hash12_the_stored_order_matches_the_rust_side(self) -> None:
        # The cross-language pin. rust/tests/hash_order.rs asserts that
        # Hash256::decode(DISPLAY).0 is exactly these bytes — the value the Rust
        # service binds into every bytea column. If either component changes
        # convention, one of the two fails.
        assert txid_to_bytes(DISPLAY).hex() == STORED_HEX


class TestIdentifierConversion:
    # 20 bytes, a p2pkh hash160 as it appears in the locking script.
    IDENT_HEX = "7c78584493557fac782023a4ad591b64545929d9"

    def test_hash08_an_identifier_is_not_reversed(self) -> None:
        # Unlike a txid. An identifier is bytes captured from the script as
        # written, so script order is its natural order.
        assert identifier_to_bytes(self.IDENT_HEX) == bytes.fromhex(self.IDENT_HEX)

    def test_hash09_the_identifier_conversion_round_trips(self) -> None:
        assert identifier_from_bytes(identifier_to_bytes(self.IDENT_HEX)) == self.IDENT_HEX

    def test_hash10_a_null_identifier_passes_through(self) -> None:
        # The column is nullable: a pattern need not declare an identifier.
        assert identifier_from_bytes(None) is None

    def test_hash11_a_non_hex_identifier_is_rejected(self) -> None:
        with pytest.raises(HashFormatError):
            identifier_to_bytes("zz")
