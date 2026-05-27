from merkle import create_merkle_branch


class TestMerkleProofRequirements:
    def test_api09_create_merkle_branch_returns_left_right_positions(self) -> None:
        txs = [
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        ]
        branches = create_merkle_branch(txs[1], txs)
        assert len(branches) >= 1
        assert branches[0]["pos"] in {"L", "R"}
        assert len(branches[0]["hash"]) == 64
