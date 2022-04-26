
import struct

from p2p_framework.serial import ser_vector, ser_uint256, deser_uint256, ser_compact_size, uint256_from_bytes, FromHex, deser_compact_size, deser_vector
from p2p_framework.object import CTransaction, CBlockHeader
from p2p_framework.util import hex_str_to_bytes


# Data for the merkle proof node part of the double-spend detected P2P message
class MerkleProofNode():
    def __init__(self, node=0):
        self.nodeType = 0
        self.node = node

    def deserialize(self, f):
        self.nodeType = struct.unpack("<B", f.read(1))[0]
        # Currently only type 0 is supported (it means node is always uint256)
        assert(self.nodeType == 0)
        self.node = deser_uint256(f)

    def serialize(self):
        r = b"".join((
            struct.pack("<B", self.nodeType),
            ser_uint256(self.node),))
        return r

    def __repr__(self):
        return "MerkleProofNode(type=%i node=%064x)" % (self.nodeType, self.node)


# Data for the merkle proof part of the double-spend detected P2P message
class DSMerkleProof():
    def __init__(self, txIndex=0, tx=CTransaction(), merkleRoot=0, proof=None, json_notification=None):
        if json_notification is None:
            self.txIndex = txIndex
            self.tx = tx
            self.merkleRoot = merkleRoot
            if proof is None:
                self.proof = []
            else:
                self.proof = proof
        else:
            self.txIndex = json_notification["index"]
            self.tx = FromHex(CTransaction(), json_notification["txOrId"])
            # Only merkleRoot target type is currently supported
            assert(json_notification["targetType"] == "merkleRoot")
            self.merkleRoot = uint256_from_bytes(hex_str_to_bytes(json_notification["target"])[::-1])
            self.proof = []
            for node in json_notification["nodes"]:
                self.proof.append(MerkleProofNode(uint256_from_bytes(hex_str_to_bytes(node)[::-1])))

    def deserialize(self, f):
        flags = struct.unpack("<B", f.read(1))[0]
        # Should always be 5
        assert(flags == 5)
        self.txIndex = deser_compact_size(f)
        # Length of transaction bytes is deserialized as required by the specification, but we don't actually need it to deserialize the transaction
        deser_compact_size(f)
        self.tx = CTransaction()
        self.tx.deserialize(f)
        self.merkleRoot = deser_uint256(f)
        self.proof = deser_vector(f, MerkleProofNode)

    def serialize(self):
        txSerialized = self.tx.serialize()
        r = b"".join((
            struct.pack("<B", 5),
            ser_compact_size(self.txIndex),
            ser_compact_size(len(txSerialized)),
            txSerialized,
            ser_uint256(self.merkleRoot),
            ser_vector(self.proof),))
        return r

    def __repr__(self):
        return "DSMerkleProof(txIndex=%i tx=%s merkleRoot=%064x proof=%s)" % (self.txIndex, repr(self.tx), self.merkleRoot, repr(self.proof))


# Data for the block details part of the double-spend detected P2P message
class BlockDetails():
    def __init__(self, blockHeaders=None, merkleProof=DSMerkleProof(), json_notification=None):
        if json_notification is None:
            if blockHeaders is None:
                self.blockHeaders = []
            else:
                self.blockHeaders = blockHeaders
            self.merkleProof = merkleProof
        else:
            self.blockHeaders = []
            for blockHeader in json_notification["headers"]:
                self.blockHeaders.append(CBlockHeader(json_notification=blockHeader))
            self.merkleProof = DSMerkleProof(json_notification=json_notification["merkleProof"])

    def deserialize(self, f):
        self.blockHeaders = deser_vector(f, CBlockHeader)
        self.merkleProof = DSMerkleProof()
        self.merkleProof.deserialize(f)

    def serialize(self):
        r = b"".join((
            ser_vector(self.blockHeaders),
            self.merkleProof.serialize(),))
        return r

    def __repr__(self):
        return "BlockDetails(blockHeaders=%s merkleProof=%s)" % (repr(self.blockHeaders), repr(self.merkleProof))


# Double-spend detected P2P message
class msg_dsdetected():
    command = b"dsdetected"

    def __init__(self, version=1, blocksDetails=None, json_notification=None):
        if (json_notification is None):
            self.version = version
            if blocksDetails is None:
                self.blocksDetails = []
            else:
                self.blocksDetails = blocksDetails
        else:
            self.version = json_notification["version"]
            self.blocksDetails = []
            for json_blockDetails in json_notification["blocks"]:
                self.blocksDetails.append(BlockDetails(json_notification=json_blockDetails))

    def deserialize(self, f):
        self.version = struct.unpack("<H", f.read(2))[0]
        self.blocksDetails = deser_vector(f, BlockDetails)

    def serialize(self):
        r = b"".join((
            struct.pack("<H", self.version),
            ser_vector(self.blocksDetails),))
        return r

    def __repr__(self):
        return "msg_dsdetected(version=%i blocksDetails=%s)" % (self.version, repr(self.blocksDetails))
