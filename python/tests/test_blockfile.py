import os
import tempfile

from blockfile import load_block_at_offset
from p2p_framework.hash import hash256


class TestBlockFileRequirements:
    def test_data04_block_offset_locates_serialized_block(self) -> None:
        # Minimal synthetic block: header fields + varint(0) tx count
        version = (1).to_bytes(4, "little")
        prev_hash = b"\x01" * 32
        merkle_root = hash256(b"merkle-test")
        timestamp = (1_700_000_000).to_bytes(4, "little")
        bits = (0x1D00FFFF).to_bytes(4, "little")
        nonce = (0).to_bytes(4, "little")
        tx_count = b"\x00"
        block_bytes = version + prev_hash + merkle_root + timestamp + bits + nonce + tx_count

        with tempfile.NamedTemporaryFile(delete=False) as handle:
            handle.write(block_bytes)
            temp_path = handle.name

        try:
            block = load_block_at_offset(temp_path, 0)
            assert block.hash is not None
            assert len(block.vtx) == 0
        finally:
            os.remove(temp_path)
