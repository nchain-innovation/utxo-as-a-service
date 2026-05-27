import pytest

from util import address_to_public_key_hash

TESTNET_ADDRESS = "mgzhRq55hEYFgyCrtNxEsP1MdusZZ31hH5"
TESTNET_PUBKEYHASH = "10375cfe32b917cd24ca1038f824cd00f7391859"


class TestAddressToPublicKeyHash:
    def test_decodes_testnet_p2pkh_address(self) -> None:
        assert address_to_public_key_hash(TESTNET_ADDRESS).hex() == TESTNET_PUBKEYHASH

    def test_rejects_invalid_checksum(self) -> None:
        with pytest.raises(ValueError, match="bad address"):
            address_to_public_key_hash("mgzhRq55hEYFgyCrtNxEsP1MdusZZ31hH6")
