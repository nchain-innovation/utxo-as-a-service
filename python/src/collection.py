from typing import Dict, Any, List
from pydantic import BaseModel, field_validator
import toml

from validation import (
    validate_monitor_name,
    validate_locking_script_pattern,
)

from io import BytesIO
from p2p_framework.object import CTransaction

from database import database
from tx_analyser import tx_analyser
from config import ConfigType


def hexstr_to_tx(hash: str, hexstr: str) -> Dict[str, Any]:
    bytes = bytearray.fromhex(hexstr)
    transaction = CTransaction()
    transaction.deserialize(BytesIO(bytes))
    transaction.rehash()
    # Decode tx
    decode = tx_analyser.decode_tx(hash, transaction)
    print("decode = ", decode)
    return decode


# This represents an address or locking script monitor
class Monitor(BaseModel):
    name: str
    track_descendants: bool
    address: None | str
    locking_script_pattern: None | str

    @field_validator("name")
    @classmethod
    def check_name(cls, value: str) -> str:
        return validate_monitor_name(value)

    @field_validator("locking_script_pattern")
    @classmethod
    def check_locking_script_pattern(cls, value: None | str) -> None | str:
        if value is None:
            return value
        return validate_locking_script_pattern(value)


def load_dynamic_config(config: ConfigType) -> List[str]:
    """ Load dynamic config from provided toml file
    """
    filename = config['dynamic_config']['filename']
    # Read file
    try:
        with open(filename, "r") as f:
            config = toml.load(f)
    except FileNotFoundError as e:
        print(f"load_config - File not found error {e}")
        return []
    else:
        # Read in name fields
        return list(map(lambda x: x['name'], config['collection']))


class Collection:
    def __init__(self):
        self.static_names: List[str]
        self.dynamic_names: List[str]

    def set_config(self, config: ConfigType):
        try:
            self.static_names = list(map(lambda x: x['name'], config['collection']))
        except KeyError:
            self.static_names = []
        self.dynamic_names = load_dynamic_config(config)

    def get_collections(self) -> Dict[str, Any]:
        all_names = self.static_names + self.dynamic_names
        """ Return a list of named collections """
        return {
            "collections": all_names,
        }

    def get_tx_as_hex(self, hash: str) -> List[Any]:
        # Read tx from database
        return database.query("SELECT tx FROM collection WHERE hash = %s;", (hash,))

    def is_valid_collection(self, cname: str) -> bool:
        return cname in self.static_names or cname in self.dynamic_names

    def get_collection_contents(self, monitor_name: str) -> List[Any]:
        """ Return the collection hashes associated with this collection name """
        assert self.is_valid_collection(monitor_name)
        return database.query(
            "SELECT hash FROM collection WHERE name = %s;",
            (monitor_name,),
        )

    def add_monitor(self, monitor: Monitor):
        assert not self.is_valid_collection(monitor.name)
        self.dynamic_names.append(monitor.name)

    def delete_monitor(self, monitor_name: str):
        assert self.is_valid_collection(monitor_name)
        assert self.is_valid_dynamic_collection(monitor_name)
        self.dynamic_names.remove(monitor_name)

    def is_valid_dynamic_collection(self, monitor_name: str) -> bool:
        return monitor_name in self.dynamic_names


collection = Collection()
