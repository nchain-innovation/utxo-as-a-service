from typing import Dict, Any, MutableMapping, List

from io import BytesIO
from p2p_framework.object import CTransaction

from database import database
from tx_analyser import tx_analyser


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

    def get_raw_tx(self, cname: str, hash: str) -> Dict[str, Any]:
        # Read tx from database
        result = database.query(f"SELECT tx FROM {cname} WHERE hash = '{hash}';")
        if len(result) > 0:
            return {"result": result[0][0]}
        else:
            return {
                "failed": f"Unknown txid {hash}",
            }

    def get_raw_tx_from_collection(self, cname: str, hash: str) -> Dict[str, Any]:
        """ Return the raw tx from the named collection"""
        if cname in self.names:
            return self.get_raw_tx(cname, hash)
        else:
            return {
                "failed": f"Unknown collection {cname}",
            }

    def get_parsed_tx_from_collection(self, cname: str, hash: str) -> Dict[str, Any]:
        result = self.get_raw_tx_from_collection(cname, hash)
        if "failed" in result:
            return result
        else:
            # tx_hexstr -> CTransaction
            bytes = bytearray.fromhex(result["result"])
            transaction = CTransaction()
            transaction.deserialize(BytesIO(bytes))
            transaction.rehash()
            # Decode tx
            decode = tx_analyser.decode_tx(hash, transaction)
            print("decode = ", decode)
            return decode

    def get_collection_contents(self, cname: str) -> Dict[str, Any]:
        """ Return the collection contents associated with this collection name """
        if cname in self.names:
            result = database.query(f"SELECT * FROM {cname};")
            if len(result) > 0:
                return {"result": result}
            else:
                return {
                    "failed": "Failed to access collection",
                }
        else:
            return {
                "failed": f"Unknown collection {cname}",
            }


collection = Collection()
