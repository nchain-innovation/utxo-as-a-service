from typing import Dict, Any, MutableMapping, List

from io import BytesIO
from p2p_framework.object import CTransaction

from database import database
from tx_analyser import tx_analyser


def hexstr_to_tx(hash: str, hexstr: str) -> Dict[str, Any]:
    bytes = bytearray.fromhex(hexstr)
    transaction = CTransaction()
    transaction.deserialize(BytesIO(bytes))
    transaction.rehash()
    # Decode tx
    decode = tx_analyser.decode_tx(hash, transaction)
    print("decode = ", decode)
    return decode


class Collection:
    def __init__(self):
        self.names: List[str]

    def set_config(self, config: MutableMapping[str, Any]):
        try:
            self.names = list(map(lambda x: x['name'], config['collection']))
        except KeyError:
            self.names = []

    def get_collections(self) -> Dict[str, Any]:
        """ Return a list of named collections """
        return {
            "collections": self.names,
        }

    def get_raw_tx(self, hash: str) -> List[Any]:
        # Read tx from database
        return database.query(f"SELECT tx FROM collection WHERE hash = '{hash}';")

    def is_valid_collection(self, cname: str) -> bool:
        return cname in self.names

    def get_collection_contents(self, cname: str) -> List[Any]:
        """ Return the collection hashes associated with this collection name """
        assert self.is_valid_collection(cname)
        return database.query(f"SELECT hash FROM collection WHERE name = '{cname}';")


collection = Collection()
