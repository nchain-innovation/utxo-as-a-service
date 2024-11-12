from fastapi import FastAPI, status, Response
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel
from typing import Any, Dict
import requests
from io import BytesIO

from p2p_framework.object import CTransaction
from tx_engine import address_to_public_key_hash

from config import load_config, ConfigType
from tx_analyser import tx_analyser
from block_manager import block_manager
from collection import collection, hexstr_to_tx, Monitor
from logic import logic

tags_metadata = [
    {
        "name": "UTXO as a Service (UaaS) REST API",
        "description": "UTXO as a Service REST API",
    },
]


app = FastAPI(
    title="UTXO as a Service (UaaS) REST API",
    description="UTXO as a Service REST API",
    openapi_tags=tags_metadata,
)

# Enable CORS
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

config: ConfigType = load_config("../data/uaasr.toml")
web_address: str = config["web_interface"]["address"]
rust_url: str = config["web_interface"]["rust_url"]


@app.get("/", tags=["Web"])
def root() -> Dict[str, str]:
    """Web server Root"""
    return {
        "name": "UTXO as a Service (UaaS) REST API",
        "description": "UTXO as a Service REST API",
    }


# Service Status
@app.get("/status", tags=["Status"])
def get_status() -> Dict[str, Any]:
    """ Return the current service status """
    return logic.get_status()

# Tx


if config[config["service"]["network"]]["save_blocks"]:
    # Can only get this info if we have saved the blocks
    @app.get("/tx", tags=["Tx"])
    def get_transaction(hash: str) -> Dict[str, Any]:
        """ Return the transaction entry identified by hash as a dictionary
            Note that this also indicates if the transaction outpoints have been spent or not
        """
        return tx_analyser.get_tx_entry(hash)

    # Can only get this info if we have saved the blocks
    @app.get("/tx/hex", tags=["Tx"])
    def get_tx_hex(hash: str) -> Dict[str, Any]:
        """ Return the tx raw entry identified by hash"""
        return tx_analyser.get_tx_raw_entry(hash)

else:
    """ Note that if we are not saving Tx we can only get txs from the collections
    """
    @app.get("/tx", tags=["Tx"])
    def get_parsed_tx_from_collection(hash: str, response: Response) -> Dict[str, Any]:
        """ Return the tx from the  collection
            Note that this also indicates if the transaction outpoints have been spent or not
        """
        result = collection.get_tx_as_hex(hash)
        if len(result) > 0:
            hexstr = result[0][0]
            return hexstr_to_tx(hash, hexstr)
        else:
            response.status_code = status.HTTP_422_UNPROCESSABLE_ENTITY
            return {
                "failed": f"Unknown txid {hash}",
            }

    @app.get("/tx/hex", tags=["Tx"])
    def get_tx_from_collection_as_hex(hash: str, response: Response) -> Dict[str, Any]:
        """ Return the tx hex str from the collection"""
        result = collection.get_tx_as_hex(hash)
        if len(result) > 0:
            return {"result": result[0][0]}
        else:
            response.status_code = status.HTTP_422_UNPROCESSABLE_ENTITY
            return {
                "failed": f"Unknown txid {hash}",
            }


@app.get("/tx/proof", tags=["Tx"])
def get_merkle_proof(hash: str) -> Dict[str, Any]:
    """ Return the merkle branch proof for a confirmed transaction
    """
    return tx_analyser.get_tx_merkle_proof(hash)


class Tx(BaseModel):
    tx: str


@app.post("/tx/hex", tags=["Tx"])
def broadcast_tx_hex(tx: Tx, response: Response) -> Dict[str, Any]:
    """ Broadcast the provided hex string transaction to the network"""
    # tx -> hash
    bytes = bytearray.fromhex(tx.tx)
    transaction = CTransaction()
    transaction.deserialize(BytesIO(bytes))
    transaction.rehash()
    hash = transaction.hash
    assert isinstance(hash, str)
    # CTransaction
    if tx_analyser.tx_exist(hash):
        print(f" Transaction {hash} already exists.")
        response.status_code = status.HTTP_422_UNPROCESSABLE_ENTITY
        return {"failure": f" Transaction {hash} already exists."}
    try:
        result = requests.post(rust_url + "/tx/raw", data=tx.tx)
    except requests.exceptions.ConnectionError as e:
        response.status_code = status.HTTP_503_SERVICE_UNAVAILABLE
        print(f"failure = {str(e)}")
        return {"failure": "Unable to connect with Rust service"}
    except requests.exceptions.RequestException as e:
        response.status_code = status.HTTP_503_SERVICE_UNAVAILABLE
        return {"failure": str(e)}
    else:
        print(result.status_code)
        print(result.text)
        if result.status_code == 200:
            return result.json()
        else:
            response.status_code = result.status_code
            return {"failure": result.text}


# UTXO

@app.get("/utxo/get", tags=["UTXO"])
def get_utxo(address: str, response: Response) -> Dict[str, Any]:
    """ Return the UTXO associated with a particular address"""
    # address -> pubkeyhash
    try:
        pubkeyhash = address_to_public_key_hash(address).hex()
    except RuntimeError as e:
        response.status_code = status.HTTP_422_UNPROCESSABLE_ENTITY
        return {"failure": f"Unable to decode address {address}.\n{e}."}
    else:
        return tx_analyser.get_utxo(pubkeyhash)


@app.get("/utxo/balance", tags=["UTXO"])
def get_balance(address: str, response: Response) -> Dict[str, Any]:
    """ Return the balance associated with a particular address"""
    # address -> pubkeyhash
    try:
        pubkeyhash = address_to_public_key_hash(address).hex()
    except RuntimeError as e:
        response.status_code = status.HTTP_422_UNPROCESSABLE_ENTITY
        return {"failure": f"Unable to decode address {address}.\n{e}."}
    else:
        height = block_manager.get_block_height()
        return tx_analyser.get_balance(pubkeyhash, height)


# Block Header

@app.get("/block/latest", tags=["Block Header"])
def get_latest_block_headers() -> Dict[str, Any]:
    """ Return the latest block headers seen by the service"""
    return block_manager.get_latest_blocks()


@app.get("/block/height", tags=["Block Header"])
def get_block_header_at_height(height: int) -> Dict[str, Any]:
    """ Return the block header at the given height"""
    return block_manager.get_block_at_height(height)


@app.get("/block/hash", tags=["Block Header"])
def get_block_header_at_hash(hash: str) -> Dict[str, Any]:
    """ Return the block header at the given hash"""
    return block_manager.get_block_at_hash(hash)


@app.get("/block/last", tags=["Block Header"])
def get_last_block_header(response: Response) -> Dict[str, Any]:
    """ Return the last block header seen by the service"""
    result = block_manager.get_last_block()
    if result is None:
        response.status_code = status.HTTP_503_SERVICE_UNAVAILABLE
        return {}
    else:
        return result


@app.get("/block/last/hex", tags=["Block Header"])
def get_last_block_header_as_hex(response: Response) -> Dict[str, Any]:
    """ Return the last block seen by the service as hex"""
    result = block_manager.get_last_block_as_hex()
    if result is None:
        response.status_code = status.HTTP_503_SERVICE_UNAVAILABLE
        return {}
    else:
        return result


@app.get("/collection", tags=["Collection"])
def get_collections() -> Dict[str, Any]:
    """ Return the collections associated with this service"""
    return collection.get_collections()


@app.post("/collection/monitor", tags=["Collection"])
def add_monitor(monitor: Monitor, response: Response) -> Dict[str, Any]:
    """ This endpoint can accept an address monitor or locking script monitor
    """
    if monitor.address is None and monitor.locking_script_pattern is None:
        response.status_code = status.HTTP_422_UNPROCESSABLE_ENTITY
        return {
            "failed": f"Invalid monitor {monitor}",
        }
    if collection.is_valid_collection(monitor.name):
        response.status_code = status.HTTP_422_UNPROCESSABLE_ENTITY
        return {
            "failed": f"Monitor name '{monitor.name}' already exists ",
        }
    if monitor.address is None and monitor.locking_script_pattern is None:
        response.status_code = status.HTTP_422_UNPROCESSABLE_ENTITY
        return {
            "failed": f"Monitor is invalid '{monitor}'",
        }
    data = monitor.model_dump(mode='json')
    print("data=", data)
    try:
        result = requests.post(rust_url + "/collection/monitor", json=data)
    except requests.exceptions.ConnectionError as e:
        response.status_code = status.HTTP_503_SERVICE_UNAVAILABLE
        print(f"failure = {str(e)}")
        return {"failure": "Unable to connect with Rust service"}
    except requests.exceptions.RequestException as e:
        response.status_code = status.HTTP_503_SERVICE_UNAVAILABLE
        return {"failure": str(e)}
    else:
        if result.status_code == 200:
            collection.add_monitor(monitor)
            return {}
        else:
            response.status_code = result.status_code
            if result.text == "":
                return {"failure": "Unable to connect to backend"}
            else:
                return {"failure": result.text}


@app.delete("/collection/monitor", tags=["Collection"])
def delete_monitor(monitor_name: str, response: Response) -> Dict[str, Any]:
    """ This endpoint can delete an monitor with the provided address
    """
    if not collection.is_valid_collection(monitor_name):
        response.status_code = status.HTTP_422_UNPROCESSABLE_ENTITY
        return {
            "failed": f"Monitor name does not exist '{monitor_name}'",
        }

    if not collection.is_valid_dynamic_collection(monitor_name):
        response.status_code = status.HTTP_422_UNPROCESSABLE_ENTITY
        return {
            "failed": f"Monitor name is not a valid dynmatic monitor '{monitor_name}'",
        }

    # call uaas backend
    try:
        result = requests.delete(rust_url + f"/collection/monitor/{monitor_name}")
    except requests.exceptions.ConnectionError as e:
        response.status_code = status.HTTP_503_SERVICE_UNAVAILABLE
        print(f"failure = {str(e)}")
        return {"failure": "Unable to connect with Rust service"}
    except requests.exceptions.RequestException as e:
        response.status_code = status.HTTP_503_SERVICE_UNAVAILABLE
        return {"failure": str(e)}
    else:
        if result.status_code == 200:
            collection.delete_monitor(monitor_name)
            return {}
        else:
            response.status_code = result.status_code
            if result.text == "":
                return {"failure": "Unable to connect to backend"}
            else:
                return {"failure": result.text}
