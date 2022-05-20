from typing import Dict, Any, MutableMapping, List

from database import database


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


collection = Collection()
