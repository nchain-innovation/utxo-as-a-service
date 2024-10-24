from fastapi import FastAPI, status, Response
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel
from typing import Any, Dict
import requests
from io import BytesIO

from p2p_framework.object import CTransaction

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


if config[config["service"]["network"]]["save_blocks"]:
    # Can only get this info if we have saved the blocks
    @app.get("/tx", tags=["Tx"])
    def get_transaction(hash: str) -> Dict[str, Any]:
        """ Return the transaction entry identified by hash as a dictionary
            Note that this also indicates if the transaction outpoints have been spent or not
        """
        return tx_analyser.get_tx_entry(hash)

    # Can only get this info if we have saved the blocks
    @app.get("/tx/raw", tags=["Tx"])
    def get_tx_raw(hash: str) -> Dict[str, Any]:
        """ Return the tx raw entry identified by hash"""
        return tx_analyser.get_tx_raw_entry(hash)


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


@app.get("/tx/utxo", tags=["Tx"])
def get_utxo(hash: str) -> Dict[str, Any]:
    """ Return the utxo entry identified by hash"""
    return tx_analyser.get_utxo_entry(hash)


@app.get("/tx/utxo_by_outpoint", tags=["Tx"])
def get_utxo_by_outpoint(hash: str, pos: int) -> Dict[str, Any]:
    """ Return the utxo entry identified by hash and pos"""
    return tx_analyser.get_utxo_by_outpoint(hash, pos)


@app.get("/block/latest", tags=["Block"])
def get_latest_blocks() -> Dict[str, Any]:
    """ Return the latest blocks seen by the service"""
    return block_manager.get_latest_blocks()


@app.get("/block/height", tags=["Block"])
def get_block_at_height(height: int) -> Dict[str, Any]:
    """ Return the block at the given height"""
    return block_manager.get_block_at_height(height)


@app.get("/block/hash", tags=["Block"])
def get_block_at_hash(hash: str) -> Dict[str, Any]:
    """ Return the block at the given hash"""
    return block_manager.get_block_at_hash(hash)


@app.get("/block/last", tags=["Block"])
def get_last_block(response: Response) -> Dict[str, Any]:
    """ Return the last block seen by the service"""
    result = block_manager.get_last_block()
    if result is None:
        response.status_code = status.HTTP_503_SERVICE_UNAVAILABLE
        return {}
    else:
        return result


@app.get("/block/last/hex", tags=["Block"])
def get_last_block_hex(response: Response) -> Dict[str, Any]:
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


@app.get("/collection/contents", tags=["Collection"])
def get_collection_contents(cname: str, response: Response) -> Dict[str, Any]:
    """ Return the collection hashes associated with this collection name
    """
    if collection.is_valid_collection(cname):
        result = collection.get_collection_contents(cname)
        if len(result) > 0:
            result = [r[0] for r in result]
            return {"result": result}
        else:
            response.status_code = status.HTTP_503_SERVICE_UNAVAILABLE
            return {
                "failed": "Failed to access collection",
            }
    else:
        response.status_code = status.HTTP_422_UNPROCESSABLE_ENTITY
        return {
            "failed": f"Unknown collection {cname}",
        }


@app.get("/collection/tx/hex", tags=["Collection"])
def get_raw_tx_from_collection(hash: str, response: Response) -> Dict[str, Any]:
    """ Return the tx hex str from the named collection"""
    result = collection.get_raw_tx(hash)
    if len(result) > 0:
        return {"result": result[0][0]}
    else:
        response.status_code = status.HTTP_422_UNPROCESSABLE_ENTITY
        return {
            "failed": f"Unknown txid {hash}",
        }


@app.get("/collection/tx/parsed", tags=["Collection"])
def get_parsed_tx_from_collection(hash: str, response: Response) -> Dict[str, Any]:
    """ Return the tx from the  collection
        Note that this also indicates if the transaction outpoints have been spent or not
    """
    result = collection.get_raw_tx(hash)
    if len(result) > 0:
        hexstr = result[0][0]
        return hexstr_to_tx(hash, hexstr)
    else:
        response.status_code = status.HTTP_422_UNPROCESSABLE_ENTITY
        return {
            "failed": f"Unknown txid {hash}",
        }


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
        result = requests.delete(rust_url + f"/collection/monitor?monitor_name={monitor_name}")
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
