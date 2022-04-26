
import uuid
import struct
from typing import Tuple, List, Generator, Any
from io import BytesIO
from p2p_framework.util import hex_str_to_bytes, bytes_to_hex_str
from itertools import chain
# Serialization/deserialization tools


def ser_compact_size(item: int) -> bytes:
    """ given int return bytes
    """
    r = b""
    if item < 253:
        r = struct.pack("B", item)
    elif item < 0x10000:
        r = struct.pack("<BH", 253, item)
    elif item < 0x100000000:
        r = struct.pack("<BI", 254, item)
    else:
        r = struct.pack("<BQ", 255, item)
    return r


def generator_based_serializator(fn):
    def decorated(object_collection, *args, **kwargs):
        first_elem = ser_compact_size(len(object_collection))
        obj_generator = fn(object_collection, *args, **kwargs)
        return b"".join(chain((first_elem,), obj_generator))

    return decorated


def deser_compact_size(f: BytesIO) -> int:
    nit = struct.unpack("<B", f.read(1))[0]
    if nit == 253:
        nit = struct.unpack("<H", f.read(2))[0]
    elif nit == 254:
        nit = struct.unpack("<I", f.read(4))[0]
    elif nit == 255:
        nit = struct.unpack("<Q", f.read(8))[0]
    return nit


def ser_varint(v: int) -> bytes:
    r = b""
    length = 0
    while True:
        r += struct.pack("<B", (v & 0x7F) | (0x80 if length > 0 else 0x00))
        if(v <= 0x7F):
            return r[::-1]  # Need as little-endian
        v = (v >> 7) - 1
        length += 1


def deser_varint(f: BytesIO) -> int:
    ntot = 0
    while True:
        n = struct.unpack("<B", f.read(1))[0]
        ntot = (n << 7) | (n & 0x7F)
        if((n & 0x80) == 0):
            return ntot


def deser_string(f: BytesIO) -> bytes:
    nit = deser_compact_size(f)
    return f.read(nit)


@generator_based_serializator
def ser_string(s: str) -> Tuple[str]:
    return (s,)  # return tuple with single member


def deser_uint256(f: BytesIO) -> int:
    r = 0
    for i in range(8):
        t = struct.unpack("<I", f.read(4))[0]
        r += t << (i * 32)
    return r


def ser_uint256(u: int) -> bytes:
    rs = b""
    for i in range(8):
        rs += struct.pack("<I", u & 0xFFFFFFFF)
        u >>= 32
    return rs


def uint256_from_bytes(s: bytes) -> int:
    r = 0
    t = struct.unpack("<IIIIIIII", s[:32])
    for i in range(8):
        r += t[i] << (i * 32)
    return r


def uint256_from_compact(c) -> int:
    nbytes = (c >> 24) & 0xFF
    v = (c & 0xFFFFFF) << (8 * (nbytes - 3))
    return v


def deser_vector(f: BytesIO, c) -> List[Any]:
    """ Deserialise a list of things identified by c
    """
    nit = deser_compact_size(f)
    r = []
    for i in range(nit):
        t = c()
        t.deserialize(f)
        r.append(t)
    return r


# ser_function_name: Allow for an alternate serialization function on the
# entries in the vector.
@generator_based_serializator
def ser_vector(vec, ser_function_name=""):
    # using generator because of need for lazy evaluation
    return (getattr(i, ser_function_name, i.serialize)() for i in vec)


def deser_uint256_vector(f: BytesIO) -> List[int]:
    nit = deser_compact_size(f)
    r = []
    for i in range(nit):
        t = deser_uint256(f)
        r.append(t)
    return r


@generator_based_serializator
def ser_uint256_vector(vec: List[int]) -> Generator[bytes, None, None]:
    return (ser_uint256(i) for i in vec)


def deser_string_vector(f: BytesIO) -> List[bytes]:
    nit = deser_compact_size(f)
    r = []
    for i in range(nit):
        t = deser_string(f)
        r.append(t)
    return r


@generator_based_serializator
def ser_string_vector(vec: List[str]) -> Generator[Tuple[str], None, None]:
    return (ser_string(sv) for sv in vec)


def deser_int_vector(f: BytesIO) -> List[int]:
    nit = deser_compact_size(f)
    r = []
    for i in range(nit):
        t = struct.unpack("<i", f.read(4))[0]
        r.append(t)
    return r


@generator_based_serializator
def ser_int_vector(vec):
    return (struct.pack("<i", i) for i in vec)


def deser_varint_vector(f: BytesIO) -> List[int]:
    nit = deser_varint(f)
    r = []
    for i in range(nit):
        t = deser_varint(f)
        r.append(t)
    return r


def ser_varint_vector(vec: List[int]) -> bytes:
    r = ser_varint(len(vec))
    for v in vec:
        r += ser_varint(v)
    return r


def FromHex(obj, hex_string: str) -> Any:
    """ Deserialize from a hex string representation (eg from RPC)
    """
    obj.deserialize(BytesIO(hex_str_to_bytes(hex_string)))
    return obj


def ToHex(obj: Any) -> str:
    """ Convert a binary-serializable object to hex (eg for submission via RPC)
    """
    return bytes_to_hex_str(obj.serialize())


def serialise_uuid_associd(assocId) -> bytes:
    """ Serialise a UUID association ID as a stream of bytes for sending over the network
    """
    assocIdBytes = bytes()
    if(assocId):
        assocIdPlusType = b"".join((
            struct.pack("<B", 0),
            assocId.bytes
        ))
        assocIdBytes = ser_string(assocIdPlusType)
    return assocIdBytes


def deserialise_uuid_associd(raw: bytes) -> uuid.UUID:
    """ Deserialise an association ID from the network into a UUID
    """
    return uuid.UUID(bytes=raw[1:])


# Create a new random association ID
def create_association_id() -> uuid.UUID:
    return uuid.uuid4()
