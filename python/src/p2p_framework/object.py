
import struct
import socket
import time
import copy
from io import BytesIO

from codecs import encode
from p2p_framework.util import hex_str_to_bytes, bytes_to_hex_str, int_to_hex_str

from p2p_framework.serial import (
    deser_uint256, ser_uint256, deser_compact_size, ser_compact_size, deser_string, ser_string,
    deser_int_vector, ser_int_vector, ser_string_vector, deser_string_vector, deser_uint256_vector,
    ser_uint256_vector, ser_vector, deser_vector, uint256_from_bytes,
    uint256_from_compact, deser_varint_vector, ser_varint_vector)
from p2p_framework.consensus import MY_VERSION, MAX_PROTOCOL_RECV_PAYLOAD_LENGTH, COIN
from p2p_framework.hash import siphash256, hash256, sha256
# Objects that map to bitcoind objects, which can be serialized/deserialized

from typing import List, Optional, Dict, Any


class CAddressInVersion(object):
    """ The CAddressInVersion class holds the address and port of peer nodes.
        Because the nVersion field has not been passed before the VERSION message the protocol uses an old format for the CAddress (missing nTime)
        This class handles that old format
    """
    def __init__(self, ip="0.0.0.0", port=0):
        self.nServices: int = 1
        self.pchReserved: bytes = b"\x00" * 10 + b"\xff" * 2  # ip is 16 bytes on wire to handle v6
        self.ip: str = ip
        self.port: int = port

    def deserialize(self, f: BytesIO) -> None:
        self.nServices = struct.unpack("<Q", f.read(8))[0]
        self.pchReserved = f.read(12)
        self.ip = socket.inet_ntoa(f.read(4))
        self.port = struct.unpack(">H", f.read(2))[0]

    def serialize(self) -> bytes:
        r = b"".join((
            struct.pack("<Q", self.nServices),
            self.pchReserved,
            socket.inet_aton(self.ip),
            struct.pack(">H", self.port),))
        return r

    def __repr__(self) -> str:
        return "CAddressInVersion(nServices=%i ip=%s port=%i)" % (self.nServices, self.ip, self.port)


class CAddress():
    """ The CAddress class holds the address and port of peer nodes.
        Handle new-style CAddress objects (with nTime)
    """
    def __init__(self, ip="0.0.0.0", port=0):
        self.nServices: int = 1
        self.nTime: int = int(time.time())
        self.pchReserved: bytes = b"\x00" * 10 + b"\xff" * 2  # ip is 16 bytes on wire to handle v6
        self.ip: str = ip
        self.port: int = port

    def deserialize(self, f: BytesIO) -> None:
        self.nTime = struct.unpack("<L", f.read(4))[0]
        self.nServices = struct.unpack("<Q", f.read(8))[0]
        self.pchReserved = f.read(12)
        self.ip = socket.inet_ntoa(f.read(4))
        self.port = struct.unpack(">H", f.read(2))[0]

    def serialize(self) -> bytes:
        r = b""
        r += struct.pack("<L", self.nTime)
        r += struct.pack("<Q", self.nServices)
        r += self.pchReserved
        r += socket.inet_aton(self.ip)
        r += struct.pack(">H", self.port)
        return r

    def __repr__(self) -> str:
        return "CAddress(nServices=%i ip=%s port=%i time=%s)" % (self.nServices, self.ip, self.port, time.ctime(self.nTime))


class CInv():
    """ Inv type
    """
    ERROR = 0
    TX = 1
    BLOCK = 2
    COMPACT_BLOCK = 4

    typemap = {
        ERROR: "Error",
        TX: "TX",
        BLOCK: "Block",
        COMPACT_BLOCK: "CompactBlock"
    }

    def __init__(self, t=ERROR, h=0):
        self.type: int = t
        self.hash: int = h

    def deserialize(self, f: BytesIO) -> None:
        self.type = struct.unpack("<i", f.read(4))[0]
        self.hash = deser_uint256(f)

    def serialize(self) -> bytes:
        r = b"".join((
            struct.pack("<i", self.type),
            ser_uint256(self.hash),))
        return r

    def __repr__(self) -> str:
        return "CInv(type=%s hash=%064x)" \
            % (self.typemap[self.type], self.hash)

    @classmethod
    def estimateMaxInvElements(cls, max_payload_length: int = MAX_PROTOCOL_RECV_PAYLOAD_LENGTH) -> int:
        return int((max_payload_length - 8) / (4 + 32))


class CProtoconf():
    """
    Peers respond with:
    CProtoconf(
        number_of_fields=0000000000000000000000000000000000000000000000000000000000000002
        max_recv_payload_length=0000000000000000000000000000000000000000000000000000000000200000
        stream_policies=b'BlockPriority,Default'
    )
    """
    def __init__(self, number_of_fields=2, max_recv_payload_length=0x200000, stream_policies=b"Default"):
        self.number_of_fields = number_of_fields
        self.max_recv_payload_length = max_recv_payload_length
        self.stream_policies = stream_policies

    def deserialize(self, f: BytesIO) -> None:
        self.number_of_fields = deser_compact_size(f)
        self.max_recv_payload_length = struct.unpack("<i", f.read(4))[0]
        if self.number_of_fields > 1:
            self.stream_policies = deser_string(f)

    def serialize(self) -> bytes:
        r = b""
        r += ser_compact_size(self.number_of_fields)
        r += struct.pack("<i", self.max_recv_payload_length)
        if self.number_of_fields > 1:
            r += ser_string(self.stream_policies)
        return r

    def __repr__(self) -> str:
        return "CProtoconf(number_of_fields=0x%x max_recv_payload_length=0x%x stream_policies=%s)" \
            % (self.number_of_fields, self.max_recv_payload_length, self.stream_policies)


class CBlockLocator():
    """ Contains
        * a list of blockhashes
        * version of the peer
    """
    def __init__(self, have=[]):
        self.nVersion: int = MY_VERSION
        self.vHave: List[int] = have

    def deserialize(self, f: BytesIO) -> None:
        self.nVersion = struct.unpack("<i", f.read(4))[0]
        self.vHave = deser_uint256_vector(f)

    def serialize(self) -> bytes:
        r = b"".join((
            struct.pack("<i", self.nVersion),
            ser_uint256_vector(self.vHave),))
        return r

    def __repr__(self) -> str:
        vhave_str = [int_to_hex_str(x) for x in self.vHave]
        return "CBlockLocator(nVersion=%i vHave=%s)" \
            % (self.nVersion, repr(vhave_str))


class COutPoint():
    """ Transaction OutPoint - a combination of hash and index of the transaction being spent
    """
    def __init__(self, hash: int = 0, n: int = 0):
        self.hash: int = hash
        self.n: int = n

    def deserialize(self, f: BytesIO) -> None:
        self.hash = deser_uint256(f)
        self.n = struct.unpack("<I", f.read(4))[0]

    def serialize(self) -> bytes:
        r = b"".join((
            ser_uint256(self.hash),
            struct.pack("<I", self.n),))
        return r

    def __hash__(self) -> int:
        """ Make COutPoint hashable for use in dictionaries
        """
        return self.hash + self.n

    def __eq__(self, other) -> bool:
        return self.n == other.n and self.hash == other.hash

    def __repr__(self) -> str:
        return "COutPoint(hash=%064x n=%i)" % (self.hash, self.n)

    def to_dict(self) -> Dict[str, Any]:
        return {"hash": f"{self.hash:064x}", "n": self.n}

    def to_str(self) -> str:
        return f"{self.hash:064x}:{self.n}"


class CTxIn():
    def __init__(self, outpoint=None, scriptSig=b"", nSequence=0):
        self.prevout: COutPoint
        if outpoint is None:
            self.prevout = COutPoint()
        else:
            self.prevout = outpoint
        self.scriptSig: bytes = scriptSig
        self.nSequence: int = nSequence

    def deserialize(self, f: BytesIO) -> None:
        self.prevout = COutPoint()
        self.prevout.deserialize(f)
        self.scriptSig = deser_string(f)
        self.nSequence = struct.unpack("<I", f.read(4))[0]

    def serialize(self) -> bytes:
        r = b"".join((
            self.prevout.serialize(),
            ser_string(self.scriptSig),
            struct.pack("<I", self.nSequence),))
        return r

    def __repr__(self) -> str:
        return "CTxIn(prevout=%s scriptSig=%s nSequence=%i)" \
            % (repr(self.prevout), bytes_to_hex_str(self.scriptSig),
               self.nSequence)

    def to_dict(self) -> Dict[str, Any]:
        return {
            "prevout": self.prevout.to_dict(),
            "scriptSig": self.scriptSig.hex(),
            "sequence": f"{self.nSequence:04x}"
        }


class CTxOut():
    def __init__(self, nValue=0, scriptPubKey=b""):
        self.nValue: int = nValue
        self.scriptPubKey: bytes = scriptPubKey

    def deserialize(self, f: BytesIO) -> None:
        self.nValue = struct.unpack("<q", f.read(8))[0]
        self.scriptPubKey = deser_string(f)

    def serialize(self) -> bytes:
        r = b"".join((
            struct.pack("<q", self.nValue),
            ser_string(self.scriptPubKey),))
        return r

    def __repr__(self) -> str:
        return "CTxOut(nValue=%i.%08i scriptPubKey=%s)" \
            % (self.nValue // COIN, self.nValue % COIN,
               bytes_to_hex_str(self.scriptPubKey))

    def to_dict(self) -> Dict[str, Any]:
        return {
            "value": self.nValue,
            "scriptPubKey": self.scriptPubKey.hex(),
        }


class CTransaction():
    def __init__(self, tx=None):
        self.nVersion: int
        self.vin: List[CTxIn]
        self.vout: List[CTxOut]
        self.nLockTime: int
        self.sha256: Optional[int]
        self.hash: Optional[str]

        if tx is None:
            self.nVersion = 1
            self.vin = []
            self.vout = []
            self.nLockTime = 0
            self.sha256 = None
            self.hash = None
        else:
            self.nVersion = tx.nVersion
            self.vin = copy.deepcopy(tx.vin)
            self.vout = copy.deepcopy(tx.vout)
            self.nLockTime = tx.nLockTime
            self.sha256 = tx.sha256
            self.hash = tx.hash

    def deserialize(self, f: BytesIO) -> None:
        self.nVersion = struct.unpack("<i", f.read(4))[0]
        self.vin = deser_vector(f, CTxIn)
        self.vout = deser_vector(f, CTxOut)
        self.nLockTime = struct.unpack("<I", f.read(4))[0]
        self.sha256 = None
        self.hash = None

    def serialize(self) -> bytes:
        r = b"".join((
            struct.pack("<i", self.nVersion),
            ser_vector(self.vin),
            ser_vector(self.vout),
            struct.pack("<I", self.nLockTime),))
        return r

    def rehash(self) -> None:
        """ Recalculate the txid
        """
        self.sha256 = None
        self.calc_sha256()

    def calc_sha256(self) -> None:
        """ self.sha256 and self.hash -- those are expected to be the txid.
        """
        if self.sha256 is None:
            self.sha256 = uint256_from_bytes(hash256(self.serialize()))
        self.hash = encode(
            hash256(self.serialize())[::-1], 'hex_codec').decode('ascii')

    def is_valid(self) -> bool:
        self.calc_sha256()
        for tout in self.vout:
            if tout.nValue < 0 or tout.nValue > 21000000 * COIN:
                return False
        return True

    def __repr__(self) -> str:
        self.rehash()
        return "CTransaction(hash=%s nVersion=%i vin=%s vout=%s nLockTime=%i)" \
            % (self.hash, self.nVersion, repr(self.vin), repr(self.vout), self.nLockTime)

    def to_dict(self) -> Dict[str, Any]:
        return {
            "version": self.nVersion,
            "vin": [vin.to_dict() for vin in self.vin],
            "vout": [vout.to_dict() for vout in self.vout],
            "locktime": self.nLockTime
        }


class CBlockHeader():
    def __init__(self, header=None, json_notification=None):
        self.nVersion: int
        self.hashPrevBlock: int
        self.hashMerkleRoot: int
        self.nTime: int
        self.nBits: int  # number of 0s in the hash to solve
        self.nNonce: int
        self.sha256: Optional[int]
        self.hash: Optional[str]

        if json_notification is None:
            if header is None:
                self.set_null()
            else:
                self.nVersion = header.nVersion
                self.hashPrevBlock = header.hashPrevBlock
                self.hashMerkleRoot = header.hashMerkleRoot
                self.nTime = header.nTime
                self.nBits = header.nBits
                self.nNonce = header.nNonce
                self.sha256 = header.sha256
                self.hash = header.hash
                self.calc_sha256()
        else:
            self.nVersion = json_notification["version"]
            self.hashPrevBlock = uint256_from_bytes(hex_str_to_bytes(json_notification["hashPrevBlock"])[::-1])
            self.hashMerkleRoot = uint256_from_bytes(hex_str_to_bytes(json_notification["hashMerkleRoot"])[::-1])
            self.nTime = json_notification["time"]
            self.nBits = json_notification["bits"]
            self.nNonce = json_notification["nonce"]
            self.rehash()

    def set_null(self) -> None:
        self.nVersion = 1
        self.hashPrevBlock = 0
        self.hashMerkleRoot = 0
        self.nTime = 0
        self.nBits = 0
        self.nNonce = 0
        self.sha256 = None
        self.hash = None

    def deserialize(self, f: BytesIO) -> None:
        self.nVersion = struct.unpack("<i", f.read(4))[0]
        self.hashPrevBlock = deser_uint256(f)
        self.hashMerkleRoot = deser_uint256(f)
        self.nTime = struct.unpack("<I", f.read(4))[0]
        self.nBits = struct.unpack("<I", f.read(4))[0]
        self.nNonce = struct.unpack("<I", f.read(4))[0]
        self.sha256 = None
        self.hash = None

    def serialize(self) -> bytes:
        r = b"".join((
            struct.pack("<i", self.nVersion),
            ser_uint256(self.hashPrevBlock),
            ser_uint256(self.hashMerkleRoot),
            struct.pack("<I", self.nTime),
            struct.pack("<I", self.nBits),
            struct.pack("<I", self.nNonce),))
        return r

    def calc_sha256(self):
        """ Calculate the sha256 and hash if not already done
        """
        if self.sha256 is None:
            r = b"".join((
                struct.pack("<i", self.nVersion),
                ser_uint256(self.hashPrevBlock),
                ser_uint256(self.hashMerkleRoot),
                struct.pack("<I", self.nTime),
                struct.pack("<I", self.nBits),
                struct.pack("<I", self.nNonce),))
            self.sha256 = uint256_from_bytes(hash256(r))
            self.hash = encode(hash256(r)[::-1], 'hex_codec').decode('ascii')

    def rehash(self) -> int:
        """ Recalculate the sha256, hash and return sha256
        """
        self.sha256 = None
        self.calc_sha256()
        assert self.sha256 is not None
        return self.sha256

    def hashPrevBlock_as_hex_str(self) -> str:
        """ Return the hashPrevBlock_as_hex_str (so it can be compared with self.hash)
        """
        return int_to_hex_str(self.hashPrevBlock)

    def __repr__(self) -> str:
        self.rehash()
        return "CBlockHeader(hash=%s nVersion=%i hashPrevBlock=%064x hashMerkleRoot=%064x nTime=%s nBits=%08x nNonce=%08x)" \
            % (self.hash, self.nVersion, self.hashPrevBlock, self.hashMerkleRoot,
               time.ctime(self.nTime), self.nBits, self.nNonce)

    def to_dict(self) -> Dict[str, Any]:
        return {
            "hash": self.hash,
            "version": f'{self.nVersion:08x}',
            "hashPrevBlock": f'{self.hashPrevBlock:064x}',
            "hashMerkleRoot": f'{self.hashMerkleRoot:064x}',
            "nTime": time.ctime(self.nTime),
            "nBits": f'{self.nBits:08x}',
            "nNonce": f'{self.nNonce:08x}'
        }


class CBlock(CBlockHeader):

    def __init__(self, header=None):
        super(CBlock, self).__init__(header)
        self.vtx: List[CTransaction] = []

    def deserialize(self, f: BytesIO) -> None:
        super(CBlock, self).deserialize(f)
        self.vtx = deser_vector(f, CTransaction)

    def serialize(self) -> bytes:
        r = b"".join((
            super(CBlock, self).serialize(),
            ser_vector(self.vtx),))
        return r

    def get_merkle_root(self, hashes: List[bytes]) -> int:
        """ Calculate the merkle root given a vector of transaction hashes
        """
        while len(hashes) > 1:
            newhashes = []
            for i in range(0, len(hashes), 2):
                i2 = min(i + 1, len(hashes) - 1)
                newhashes.append(hash256(hashes[i] + hashes[i2]))
            hashes = newhashes
        return uint256_from_bytes(hashes[0])

    def calc_merkle_root(self) -> int:
        hashes = []
        for tx in self.vtx:
            tx.calc_sha256()
            assert tx.sha256 is not None
            hashes.append(ser_uint256(tx.sha256))
        return self.get_merkle_root(hashes)

    def is_valid(self) -> bool:
        self.calc_sha256()
        target = uint256_from_compact(self.nBits)
        assert self.sha256 is not None
        if self.sha256 > target:
            return False
        for tx in self.vtx:
            if not tx.is_valid():
                return False
        if self.calc_merkle_root() != self.hashMerkleRoot:
            return False
        return True

    def solve(self) -> None:
        """ Solve the nonce for this block
        """
        self.rehash()
        target = uint256_from_compact(self.nBits)
        assert self.sha256 is not None
        while self.sha256 > target:
            self.nNonce += 1
            self.rehash()

    def __repr__(self) -> str:
        self.rehash()
        return "CBlock(hash=%s nVersion=%i hashPrevBlock=%064x hashMerkleRoot=%064x nTime=%s nBits=%08x nNonce=%08x vtx=%s)" \
            % (self.hash, self.nVersion, self.hashPrevBlock, self.hashMerkleRoot,
               time.ctime(self.nTime), self.nBits, self.nNonce, repr(self.vtx))

    def to_dict(self) -> Dict[str, Any]:
        return {
            "header": super(CBlock, self).to_dict(),
            "transactions": [tx.hash for tx in self.vtx]
        }

    def to_header(self) -> CBlockHeader:
        """ Given a block return a blockheader
        """
        header = CBlockHeader()
        header.nVersion = self.nVersion
        header.hashPrevBlock = self.hashPrevBlock
        header.hashMerkleRoot = self.hashMerkleRoot
        header.nTime = self.nTime
        header.nBits = self.nBits
        header.nNonce = self.nNonce
        header.sha256 = self.sha256
        header.hash = self.hash
        return header


class CUnsignedAlert():
    def __init__(self):
        self.nVersion: int = 1
        self.nRelayUntil = 0
        self.nExpiration = 0
        self.nID = 0
        self.nCancel = 0
        self.setCancel = []
        self.nMinVer = 0
        self.nMaxVer = 0
        self.setSubVer = []
        self.nPriority = 0
        self.strComment = b""
        self.strStatusBar = b""
        self.strReserved = b""

    def deserialize(self, f: BytesIO) -> None:
        self.nVersion = struct.unpack("<i", f.read(4))[0]
        self.nRelayUntil = struct.unpack("<q", f.read(8))[0]
        self.nExpiration = struct.unpack("<q", f.read(8))[0]
        self.nID = struct.unpack("<i", f.read(4))[0]
        self.nCancel = struct.unpack("<i", f.read(4))[0]
        self.setCancel = deser_int_vector(f)
        self.nMinVer = struct.unpack("<i", f.read(4))[0]
        self.nMaxVer = struct.unpack("<i", f.read(4))[0]
        self.setSubVer = deser_string_vector(f)
        self.nPriority = struct.unpack("<i", f.read(4))[0]
        self.strComment = deser_string(f)
        self.strStatusBar = deser_string(f)
        self.strReserved = deser_string(f)

    def serialize(self) -> bytes:
        r = b"".join((
            struct.pack("<i", self.nVersion),
            struct.pack("<q", self.nRelayUntil),
            struct.pack("<q", self.nExpiration),
            struct.pack("<i", self.nID),
            struct.pack("<i", self.nCancel),
            ser_int_vector(self.setCancel),
            struct.pack("<i", self.nMinVer),
            struct.pack("<i", self.nMaxVer),
            ser_string_vector(self.setSubVer),
            struct.pack("<i", self.nPriority),
            ser_string(self.strComment),
            ser_string(self.strStatusBar),
            ser_string(self.strReserved),))
        return r

    def __repr__(self) -> str:
        return f"CUnsignedAlert(nVersion {self.nVersion}, nRelayUntil {self.nRelayUntil}, nExpiration {self.nExpiration}, nID {self.nID}" \
            "nCancel {self.nCancel}, nMinVer {self.nMinVer}, nMaxVer {self.nMaxVer}, nPriority {self.nPriority}, strComment {self.strComment}, strStatusBar {self.strStatusBar}, strReserved {self.strReserved})"


class CAlert():
    def __init__(self):
        self.vchMsg = b""
        self.vchSig = b""

    def deserialize(self, f: BytesIO) -> None:
        self.vchMsg = deser_string(f)
        self.vchSig = deser_string(f)

    def serialize(self) -> bytes:
        r = b"".join((
            ser_string(self.vchMsg),
            ser_string(self.vchSig),))
        return r

    def __repr__(self):
        return "CAlert(vchMsg.sz %d, vchSig.sz %d)" \
            % (len(self.vchMsg), len(self.vchSig))


class PrefilledTransaction():
    def __init__(self, index=0, tx=None):
        self.index: int = index
        self.tx: CTransaction = tx

    def deserialize(self, f: BytesIO) -> None:
        self.index = deser_compact_size(f)
        self.tx = CTransaction()
        self.tx.deserialize(f)

    def serialize(self) -> bytes:
        r = b"".join((
            ser_compact_size(self.index),
            self.tx.serialize(),))
        return r

    def __repr__(self) -> str:
        return "PrefilledTransaction(index=%d, tx=%s)" % (self.index, repr(self.tx))


class P2PHeaderAndShortIDs():
    """ This is what we send on the wire, in a cmpctblock message.
    """
    def __init__(self):
        self.header: CBlockHeader = CBlockHeader()
        self.nonce: int = 0
        self.shortids_length: int = 0
        self.shortids: List[int] = []
        self.prefilled_txn_length: int = 0
        self.prefilled_txn: List[PrefilledTransaction] = []

    def deserialize(self, f: BytesIO) -> None:
        self.header.deserialize(f)
        self.nonce = struct.unpack("<Q", f.read(8))[0]
        self.shortids_length = deser_compact_size(f)
        for i in range(self.shortids_length):
            # shortids are defined to be 6 bytes in the spec, so append
            # two zero bytes and read it in as an 8-byte number
            self.shortids.append(
                struct.unpack("<Q", f.read(6) + b'\x00\x00')[0])
        self.prefilled_txn = deser_vector(f, PrefilledTransaction)
        self.prefilled_txn_length = len(self.prefilled_txn)

    def serialize(self) -> bytes:
        r = b"".join((
            self.header.serialize(),
            struct.pack("<Q", self.nonce),
            ser_compact_size(self.shortids_length),
            b"".join(struct.pack("<Q", x)[0:6] for x in self.shortids),  # We only want the first 6 bytes
            ser_vector(self.prefilled_txn),))
        return r

    def __repr__(self) -> str:
        return "P2PHeaderAndShortIDs(header=%s, nonce=%d, shortids_length=%d, shortids=%s, prefilled_txn_length=%d, prefilledtxn=%s" % (repr(self.header), self.nonce, self.shortids_length, repr(self.shortids), self.prefilled_txn_length, repr(self.prefilled_txn))


def calculate_shortid(k0: int, k1: int, tx_hash: int) -> int:
    """ Calculate the BIP 152-compact blocks shortid for a given transaction hash
    """
    expected_shortid = siphash256(k0, k1, tx_hash)
    expected_shortid &= 0x0000ffffffffffff
    return expected_shortid


class HeaderAndShortIDs():
    """ This version gets rid of the array lengths, and reinterprets the differential
        encoding into indices that can be used for lookup.
    """
    def __init__(self, p2pheaders_and_shortids=None):
        self.header: CBlockHeader = CBlockHeader()
        self.nonce: int = 0
        self.shortids: List[int] = []
        self.prefilled_txn: List[PrefilledTransaction] = []

        if p2pheaders_and_shortids is not None:
            self.header = p2pheaders_and_shortids.header
            self.nonce = p2pheaders_and_shortids.nonce
            self.shortids = p2pheaders_and_shortids.shortids
            last_index = -1
            for x in p2pheaders_and_shortids.prefilled_txn:
                self.prefilled_txn.append(
                    PrefilledTransaction(x.index + last_index + 1, x.tx))
                last_index = self.prefilled_txn[-1].index

    def to_p2p(self) -> P2PHeaderAndShortIDs:
        ret = P2PHeaderAndShortIDs()
        ret.header = self.header
        ret.nonce = self.nonce
        ret.shortids_length = len(self.shortids)
        ret.shortids = self.shortids
        ret.prefilled_txn_length = len(self.prefilled_txn)
        ret.prefilled_txn = []
        last_index = -1
        for x in self.prefilled_txn:
            ret.prefilled_txn.append(
                PrefilledTransaction(x.index - last_index - 1, x.tx))
            last_index = x.index
        return ret

    def get_siphash_keys(self) -> List[int]:
        header_nonce = self.header.serialize()
        header_nonce += struct.pack("<Q", self.nonce)
        hash_header_nonce_as_str = sha256(header_nonce)
        key0 = struct.unpack("<Q", hash_header_nonce_as_str[0:8])[0]
        key1 = struct.unpack("<Q", hash_header_nonce_as_str[8:16])[0]
        return [key0, key1]

    def initialize_from_block(self, block, nonce=0, prefill_list=[0]) -> None:
        """ Version 2 compact blocks use wtxid in shortids (rather than txid)
        """
        self.header = CBlockHeader(block)
        self.nonce = nonce
        self.prefilled_txn = [PrefilledTransaction(i, block.vtx[i])
                              for i in prefill_list]
        self.shortids = []
        [k0, k1] = self.get_siphash_keys()
        for i in range(len(block.vtx)):
            if i not in prefill_list:
                tx_hash = block.vtx[i].sha256
                self.shortids.append(calculate_shortid(k0, k1, tx_hash))

    def __repr__(self) -> str:
        return "HeaderAndShortIDs(header=%s, nonce=%d, shortids=%s, prefilledtxn=%s" % (repr(self.header), self.nonce, repr(self.shortids), repr(self.prefilled_txn))


class CallbackMessage():
    """ callback message for dsnt-enabled transactions
    """
    # 127.0.0.1 as network-order bytes
    LOCAL_HOST_IP = 0x7F000001
    MAX_INT64 = 0xFFFFFFFFFFFFFFFF
    IPv6_version = 129
    IPv4_version = 1

    def __init__(self, version=1, ip_addresses=[LOCAL_HOST_IP], inputs=[0]):
        self.version: int = version
        self.ip_addresses: List[int] = ip_addresses
        self.ip_address_count: int = len(ip_addresses)
        self.inputs: List[int] = inputs

    def ser_addrs(self, addrs) -> bytes:
        rs = b""
        for addr in addrs:
            if (self.version == self.IPv6_version):
                rs += struct.pack('>QQ', (addr >> 64) & self.MAX_INT64, addr & self.MAX_INT64)
            else:
                rs += struct.pack("!I", addr)
        return rs

    def deser_addrs(self, f: BytesIO) -> List[int]:
        addrs = []
        for i in range(self.ip_address_count):
            if (self.version == self.IPv6_version):
                a, b = struct.unpack('>QQ', f.read(16))
                unpacked = (a << 64) | b
                addrs.append(unpacked)
            else:
                addrs.append(struct.unpack("!I", f.read(4))[0])
        return addrs

    def deserialize(self, f: BytesIO) -> None:
        self.version = struct.unpack("<B", f.read(1))[0]
        self.ip_address_count = deser_compact_size(f)
        self.ip_addresses = self.deser_addrs(f)
        self.inputs = deser_varint_vector(f)

    def serialize(self) -> bytes:
        r = b""
        r += struct.pack("<B", self.version)
        r += ser_compact_size(self.ip_address_count)
        r += self.ser_addrs(self.ip_addresses)
        r += ser_varint_vector(self.inputs)
        return r


class BlockTransactionsRequest():

    def __init__(self, blockhash=0, indexes=None):
        self.blockhash: int = blockhash
        self.indexes: List[int]
        self.indexes = indexes if indexes is not None else []

    def deserialize(self, f: BytesIO) -> None:
        self.blockhash = deser_uint256(f)
        indexes_length = deser_compact_size(f)
        for i in range(indexes_length):
            self.indexes.append(deser_compact_size(f))

    def serialize(self) -> bytes:
        r = b"".join((
            ser_uint256(self.blockhash),
            ser_compact_size(len(self.indexes)),
            b"".join(ser_compact_size(x) for x in self.indexes)))
        return r

    def from_absolute(self, absolute_indexes: List[int]) -> None:
        """ Helper to set the differentially encoded indexes from absolute ones
        """
        self.indexes = []
        last_index = -1
        for x in absolute_indexes:
            self.indexes.append(x - last_index - 1)
            last_index = x

    def to_absolute(self) -> List[int]:
        absolute_indexes = []
        last_index = -1
        for x in self.indexes:
            absolute_indexes.append(x + last_index + 1)
            last_index = absolute_indexes[-1]
        return absolute_indexes

    def __repr__(self) -> str:
        return "BlockTransactionsRequest(hash=%064x indexes=%s)" % (self.blockhash, repr(self.indexes))


class BlockTransactions():

    def __init__(self, blockhash=0, transactions=None):
        self.blockhash: int = blockhash
        self.transactions: List[CTransaction] = transactions if transactions is not None else []

    def deserialize(self, f: BytesIO) -> None:
        self.blockhash = deser_uint256(f)
        self.transactions = deser_vector(f, CTransaction)

    def serialize(self) -> bytes:
        r = b"".join((
            ser_uint256(self.blockhash),
            ser_vector(self.transactions),))
        return r

    def __repr__(self) -> str:
        return "BlockTransactions(hash=%064x transactions=%s)" % (self.blockhash, repr(self.transactions))
