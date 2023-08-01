from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from typing import Any, Dict
import requests
from io import BytesIO

from p2p_framework.object import CTransaction

from util import load_config, ConfigType
from address_manager import address_manager
from tx_analyser import tx_analyser
from block_manager import block_manager
from collection import collection
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


@app.get("/addr", tags=["Addresses"])
def get_addr() -> Dict[str, Any]:
    """ Return the peer addresses seen by the service"""
    return address_manager.get_peers()


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


@app.post("/tx/raw", tags=["Tx"])
def broadcast_tx_raw(tx: str) -> Dict[str, Any]:
    """ Broadcast the provided transaction to the network"""
    # tx -> hash
    bytes = bytearray.fromhex(tx)
    transaction = CTransaction()
    transaction.deserialize(BytesIO(bytes))
    transaction.rehash()
    hash = transaction.hash
    assert isinstance(hash, str)
    # CTransaction
    if tx_analyser.tx_exist(hash):
        print(f"failure: Transaction {hash} already exists.")
        return {"failure": f" Transaction {hash} already exists."}
    
    try:
        result = requests.post(rust_url + "/tx/raw", data=tx)
    except requests.exceptions.ConnectionError as e:
        print(f"failure = {str(e)}")
        return {"failure": "Unable to connect with Rust service"}
    except requests.exceptions.RequestException as e:
        return {"failure": str(e)}
    else:
        print(result.status_code)
        print(result.text)
        if result.status_code == 200:
            return result.json()
        else:
            return {"failure": result.text}

@app.get("/tx/mempool", tags=["Tx"])
def get_mempool() -> Dict[str, Any]:
    """ Return the mempool seen by the service"""
    return tx_analyser.get_mempool()


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


@app.get("/collection", tags=["Collection"])
def get_collections() -> Dict[str, Any]:
    """ Return the collections associated with this service"""
    return collection.get_collections()


@app.get("/collection/contents", tags=["Collection"])
def get_collection_contents(cname: str) -> Dict[str, Any]:
    """ Return the collection contents associated with this collection name """
    return collection.get_collection_contents(cname)


@app.get("/collection/tx/raw", tags=["Collection"])
def get_raw_tx_from_collection(cname: str, hash: str) -> Dict[str, Any]:
    """ Return the raw tx from the named collection"""
    return collection.get_raw_tx_from_collection(cname, hash)


@app.get("/collection/tx/parsed", tags=["Collection"])
def get_parsed_tx_from_collection(cname: str, hash: str) -> Dict[str, Any]:
    """ Return the tx from the named collection
        Note that this also indicates if the transaction outpoints have been spent or not
    """
    return collection.get_parsed_tx_from_collection(cname, hash)
