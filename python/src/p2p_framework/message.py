
import time
import random
import struct
from io import BytesIO
import uuid
from typing import Optional, List

from p2p_framework.object import CAddressInVersion, CProtoconf, CTransaction, CInv, CAddress, CAlert, CBlockLocator, P2PHeaderAndShortIDs, CBlock, CBlockHeader, BlockTransactionsRequest, BlockTransactions
from p2p_framework.consensus import MY_SUBVERSION, MY_VERSION, MY_RELAY
from p2p_framework.serial import create_association_id, deser_string, ser_string, deserialise_uuid_associd, serialise_uuid_associd, deser_vector, ser_vector, deser_uint256, ser_uint256
from p2p_framework.streams import StreamType


# These classes correspond to messages on the wire
class msg_version():
    command = b"version"

    def __init__(self):
        self.nVersion: int = MY_VERSION
        self.nServices: int = 1
        self.nTime: int = int(time.time())
        self.addrTo: CAddressInVersion = CAddressInVersion()
        self.addrFrom: Optional[CAddressInVersion] = CAddressInVersion()
        self.nNonce: Optional[int] = random.getrandbits(64)
        self.strSubVer: Optional[bytes] = MY_SUBVERSION
        self.nStartingHeight: Optional[int] = -1
        self.nRelay: int = MY_RELAY
        self.assocID: Optional[uuid.UUID] = create_association_id()

    def deserialize(self, f: BytesIO) -> None:
        self.nVersion = struct.unpack("<i", f.read(4))[0]
        if self.nVersion == 10300:
            self.nVersion = 300
        self.nServices = struct.unpack("<Q", f.read(8))[0]
        self.nTime = struct.unpack("<q", f.read(8))[0]
        self.addrTo = CAddressInVersion()
        self.addrTo.deserialize(f)

        if self.nVersion >= 106:
            self.addrFrom = CAddressInVersion()
            self.addrFrom.deserialize(f)
            self.nNonce = struct.unpack("<Q", f.read(8))[0]
            self.strSubVer = deser_string(f)
        else:
            self.addrFrom = None
            self.nNonce = None
            self.strSubVer = None
            self.nStartingHeight = None

        if self.nVersion >= 209:
            self.nStartingHeight = struct.unpack("<i", f.read(4))[0]
        else:
            self.nStartingHeight = None

        if self.nVersion >= 70001:
            # Relay field is optional for version 70001 onwards
            try:
                self.nRelay = struct.unpack("<b", f.read(1))[0]
                try:
                    uuidBytes = deser_string(f)
                    self.assocID = deserialise_uuid_associd(uuidBytes)
                except:
                    self.assocID = None
            except:
                self.nRelay = 0
        else:
            self.nRelay = 0
            self.assocID = None

    def serialize(self) -> bytes:
        assert self.addrFrom is not None

        r = b"".join((
            struct.pack("<i", self.nVersion),
            struct.pack("<Q", self.nServices),
            struct.pack("<q", self.nTime),
            self.addrTo.serialize(),
            self.addrFrom.serialize(),
            struct.pack("<Q", self.nNonce),
            ser_string(self.strSubVer),
            struct.pack("<i", self.nStartingHeight),
            struct.pack("<b", self.nRelay),
            serialise_uuid_associd(self.assocID),
        ))
        return r

    def __repr__(self) -> str:
        assert self.addrFrom is not None
        assert self.assocID is not None
        assert self.nNonce is not None
        assert self.nStartingHeight is not None
        return 'msg_version(nVersion=%i nServices=%i nTime=%s addrTo=%s addrFrom=%s nNonce=0x%016X strSubVer=%r nStartingHeight=%i nRelay=%i assocID=%s)' \
            % (self.nVersion, self.nServices, time.ctime(self.nTime),
               repr(self.addrTo), repr(self.addrFrom), self.nNonce,
               self.strSubVer, self.nStartingHeight, self.nRelay, str(self.assocID))


class msg_verack():
    """ Acknowledge a version message.
        This message has no parameters or content.
    """
    command = b"verack"

    def __init__(self):
        pass

    def deserialize(self, f: BytesIO) -> None:
        pass

    def serialize(self) -> bytes:
        return b""

    def __repr__(self) -> str:
        return "msg_verack()"


class msg_createstream():
    command = b"createstrm"

    def __init__(self, stream_type, stream_policy=b"", assocID=None):
        self.assocID = assocID
        self.stream_type = stream_type
        self.stream_policy = stream_policy

    def deserialize(self, f: BytesIO) -> None:
        uuidBytes = deser_string(f)
        self.assocID = deserialise_uuid_associd(uuidBytes)
        self.stream_type = struct.unpack("<B", f.read(1))[0]
        self.stream_policy = deser_string(f)

    def serialize(self) -> bytes:
        return b"".join((
            serialise_uuid_associd(self.assocID),
            struct.pack("<B", self.stream_type),
            ser_string(self.stream_policy),
        ))

    def __repr__(self) -> str:
        return f"msg_createstream(assocID={str(self.assocID)} stream_type={self.stream_type} stream_policy={str(self.stream_policy)})"


class msg_streamack():
    command = b"streamack"

    def __init__(self, assocID=None, stream_type=StreamType.UNKNOWN.value):
        self.assocID = assocID
        self.stream_type = stream_type

    def deserialize(self, f: BytesIO) -> None:
        uuidBytes = deser_string(f)
        self.assocID = deserialise_uuid_associd(uuidBytes)
        self.stream_type = struct.unpack("<B", f.read(1))[0]

    def serialize(self) -> bytes:
        return b"".join((
            serialise_uuid_associd(self.assocID),
            struct.pack("<B", self.stream_type),
        ))

    def __repr__(self) -> str:
        return "msg_streamack(assocID=%s stream_type=%i)" % (str(self.assocID), self.stream_type)


class msg_protoconf():
    """ Optional message.
        See following for further details
        https://confluence.stressedsharks.com/display/BSV/Exchanging+protocol+configuration+information
    """
    command = b"protoconf"

    def __init__(self, protoconf=None):
        if protoconf is None:
            self.protoconf = CProtoconf()
        else:
            self.protoconf = protoconf

    def deserialize(self, f: BytesIO) -> None:
        self.protoconf.deserialize(f)

    def serialize(self) -> bytes:
        r = b""
        r += self.protoconf.serialize()
        return r

    def __repr__(self) -> str:
        return "msg_protoconf(protoconf=%s)" % (repr(self.protoconf))


class msg_addr():
    """ Should contain a list of peer addresses
    """
    command = b"addr"

    def __init__(self):
        self.addrs = []

    def deserialize(self, f: BytesIO) -> None:
        # print(f.hex())
        self.addrs = deser_vector(f, CAddress)

    def serialize(self) -> bytes:
        return ser_vector(self.addrs)

    def __repr__(self) -> str:
        return "msg_addr(addrs=%s)" % (repr(self.addrs))


class msg_alert():
    command = b"alert"

    def __init__(self):
        self.alert = CAlert()

    def deserialize(self, f: BytesIO) -> None:
        self.alert = CAlert()
        self.alert.deserialize(f)

    def serialize(self) -> bytes:
        return self.alert.serialize()

    def __repr__(self) -> str:
        return "msg_alert(alert=%s)" % (repr(self.alert), )


class msg_inv():
    command = b"inv"

    def __init__(self, inv=None):
        if inv is None:
            self.inv = []
        else:
            self.inv = inv

    def deserialize(self, f: BytesIO) -> None:
        self.inv = deser_vector(f, CInv)

    def serialize(self) -> bytes:
        return ser_vector(self.inv)

    def __repr__(self) -> str:
        return "msg_inv(inv=%s)" % (repr(self.inv))


class msg_getdata():
    command = b"getdata"

    def __init__(self, inv=None):
        self.inv = inv if inv is not None else []

    def deserialize(self, f: BytesIO) -> None:
        self.inv = deser_vector(f, CInv)

    def serialize(self) -> bytes:
        return ser_vector(self.inv)

    def __repr__(self) -> str:
        return "msg_getdata(inv=%s)" % (repr(self.inv))


class msg_getblocks():
    command = b"getblocks"

    def __init__(self):
        self.locator: CBlockLocator = CBlockLocator()
        self.hashstop: int = 0

    def deserialize(self, f: BytesIO) -> None:
        self.locator = CBlockLocator()
        self.locator.deserialize(f)
        self.hashstop = deser_uint256(f)

    def serialize(self) -> bytes:
        r = b"".join((
            self.locator.serialize(),
            ser_uint256(self.hashstop),))
        return r

    def __repr__(self) -> str:
        return "msg_getblocks(locator=%s hashstop=%064x)" \
            % (repr(self.locator), self.hashstop)


class msg_tx():
    command = b"tx"

    def __init__(self, tx=None):
        self.tx: CTransaction
        if tx is None:
            self.tx = CTransaction()
        else:
            self.tx = tx

    def deserialize(self, f: BytesIO) -> None:
        self.tx.deserialize(f)

    def serialize(self) -> bytes:
        return self.tx.serialize()

    def __repr__(self) -> str:
        return "msg_tx(tx=%s)" % (repr(self.tx))


class msg_block():
    command = b"block"

    def __init__(self, block=None):
        self.block: CBlock
        if block is None:
            self.block = CBlock()
        else:
            self.block = block

    def deserialize(self, f: BytesIO) -> None:
        self.block.deserialize(f)

    def serialize(self) -> bytes:
        return self.block.serialize()

    def __repr__(self) -> str:
        return "msg_block(block=%s)" % (repr(self.block))

# for cases where a user needs tighter control over what is sent over the wire
# note that the user must supply the name of the command, and the data


class msg_generic():
    def __init__(self, command, data=None):
        self.command = command
        self.data = data

    def serialize(self) -> bytes:
        return self.data

    def __repr__(self) -> str:
        return "msg_generic()"


class msg_getaddr():
    command = b"getaddr"

    def __init__(self):
        pass

    def deserialize(self, f: BytesIO) -> None:
        pass

    def serialize(self) -> bytes:
        return b""

    def __repr__(self) -> str:
        return "msg_getaddr()"


class msg_ping_prebip31():
    command = b"ping"

    def __init__(self):
        pass

    def deserialize(self, f: BytesIO) -> None:
        pass

    def serialize(self) -> bytes:
        return b""

    def __repr__(self) -> str:
        return "msg_ping() (pre-bip31)"


class msg_ping():
    command = b"ping"

    def __init__(self, nonce=0):
        self.nonce: int = nonce

    def deserialize(self, f: BytesIO) -> None:
        self.nonce = struct.unpack("<Q", f.read(8))[0]

    def serialize(self) -> bytes:
        return struct.pack("<Q", self.nonce)

    def __repr__(self) -> str:
        return "msg_ping(nonce=%08x)" % self.nonce


class msg_pong():
    command = b"pong"

    def __init__(self, nonce=0):
        self.nonce: int = nonce

    def deserialize(self, f: BytesIO) -> None:
        self.nonce = struct.unpack("<Q", f.read(8))[0]

    def serialize(self) -> bytes:
        return struct.pack("<Q", self.nonce)

    def __repr__(self) -> str:
        return "msg_pong(nonce=%08x)" % self.nonce


class msg_mempool():
    command = b"mempool"

    def __init__(self):
        pass

    def deserialize(self, f: BytesIO) -> None:
        pass

    def serialize(self) -> bytes:
        return b""

    def __repr__(self) -> str:
        return "msg_mempool()"


class msg_sendheaders():
    command = b"sendheaders"

    def __init__(self):
        pass

    def deserialize(self, f: BytesIO) -> None:
        pass

    def serialize(self) -> bytes:
        return b""

    def __repr__(self) -> str:
        return "msg_sendheaders()"


class msg_getheaders():
    """ getheaders message has
        # number of entries
        # vector of hashes
        # hash_stop (hash of last desired block header, 0 to get as many as possible)
    """
    command = b"getheaders"

    def __init__(self, locator_have=[]):
        self.locator: CBlockLocator = CBlockLocator(locator_have)
        self.hashstop: int = 0

    def deserialize(self, f: BytesIO) -> None:
        self.locator = CBlockLocator()
        self.locator.deserialize(f)
        self.hashstop = deser_uint256(f)

    def serialize(self) -> bytes:
        r = b"".join((
            self.locator.serialize(),
            ser_uint256(self.hashstop),))
        return r

    def __repr__(self) -> str:
        return "msg_getheaders(locator=%s, stop=%064x)" \
            % (repr(self.locator), self.hashstop)


# headers message has
# <count> <vector of block headers>
class msg_headers():
    command = b"headers"

    def __init__(self):
        self.headers: List[CBlockHeader] = []

    def deserialize(self, f: BytesIO) -> None:
        # comment in bitcoind indicates these should be deserialized as blocks
        blocks = deser_vector(f, CBlock)
        for x in blocks:
            self.headers.append(CBlockHeader(x))

    def serialize(self) -> bytes:
        blocks = [CBlock(x) for x in self.headers]
        return ser_vector(blocks)

    def __repr__(self) -> str:
        return "msg_headers(headers=%s)" % repr(self.headers)


class msg_reject():
    command = b"reject"
    REJECT_MALFORMED = 1

    def __init__(self, message=b"", code=0, reason=b"", data=0):
        self.message = message
        self.code = code
        self.reason = reason
        self.data = data

    def deserialize(self, f: BytesIO) -> None:
        self.message = deser_string(f)
        self.code = struct.unpack("<B", f.read(1))[0]
        self.reason = deser_string(f)
        if (self.code != self.REJECT_MALFORMED and (self.message == b"block" or self.message == b"tx")):
            self.data = deser_uint256(f)

    def serialize(self) -> bytes:
        r = ser_string(self.message)
        r += struct.pack("<B", self.code)
        r += ser_string(self.reason)
        if (self.code != self.REJECT_MALFORMED and (self.message == b"block" or self.message == b"tx")):
            r += ser_uint256(self.data)
        return r

    def __repr__(self) -> str:
        return "msg_reject: %s %d %s [%064x]" \
            % (self.message, self.code, self.reason, self.data)


class msg_feefilter():
    command = b"feefilter"

    def __init__(self, feerate=0):
        self.feerate: int = feerate

    def deserialize(self, f: BytesIO) -> None:
        self.feerate = struct.unpack("<Q", f.read(8))[0]

    def serialize(self) -> bytes:
        return struct.pack("<Q", self.feerate)

    def __repr__(self) -> str:
        return "msg_feefilter(feerate=%08x)" % self.feerate


class msg_sendcmpct():
    command = b"sendcmpct"

    def __init__(self, announce=False):
        self.announce: bool = announce
        self.version: int = 1

    def deserialize(self, f: BytesIO) -> None:
        self.announce = struct.unpack("<?", f.read(1))[0]
        self.version = struct.unpack("<Q", f.read(8))[0]

    def serialize(self) -> bytes:
        r = b"".join((
            struct.pack("<?", self.announce),
            struct.pack("<Q", self.version),))
        return r

    def __repr__(self) -> str:
        return "msg_sendcmpct(announce=%s, version=%lu)" % (self.announce, self.version)


class msg_cmpctblock():
    command = b"cmpctblock"

    def __init__(self, header_and_shortids=None):
        self.header_and_shortids = header_and_shortids

    def deserialize(self, f: BytesIO) -> None:
        self.header_and_shortids = P2PHeaderAndShortIDs()
        self.header_and_shortids.deserialize(f)

    def serialize(self) -> bytes:
        return self.header_and_shortids.serialize()

    def __repr__(self) -> str:
        return "msg_cmpctblock(HeaderAndShortIDs=%s)" % repr(self.header_and_shortids)


class msg_getblocktxn():
    command = b"getblocktxn"

    def __init__(self):
        self.block_txn_request: Optional[BlockTransactionsRequest] = None

    def deserialize(self, f: BytesIO) -> None:
        self.block_txn_request = BlockTransactionsRequest()
        self.block_txn_request.deserialize(f)

    def serialize(self) -> bytes:
        assert self.block_txn_request is not None
        return self.block_txn_request.serialize()

    def __repr__(self) -> str:
        return "msg_getblocktxn(block_txn_request=%s)" % (repr(self.block_txn_request))


class msg_blocktxn():
    command = b"blocktxn"

    def __init__(self):
        self.block_transactions = BlockTransactions()

    def deserialize(self, f: BytesIO) -> None:
        self.block_transactions.deserialize(f)

    def serialize(self) -> bytes:
        return self.block_transactions.serialize()

    def __repr__(self) -> str:
        return "msg_blocktxn(block_transactions=%s)" % (repr(self.block_transactions))


class msg_notfound():
    command = b"notfound"

    def __init__(self, inv=None):
        self.inv: List[CInv]
        if inv is None:
            self.inv = []
        else:
            self.inv = inv

    def deserialize(self, f: BytesIO) -> None:
        self.inv = deser_vector(f, CInv)

    def serialize(self) -> bytes:
        return ser_vector(self.inv)

    def __repr__(self) -> str:
        return "msg_notfound(inv=%s)" % (repr(self.inv))
