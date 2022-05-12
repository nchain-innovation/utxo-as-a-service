from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from typing import Any, MutableMapping, Dict

from util import load_config
from address_manager import address_manager
from tx_analyser import tx_analyser
from block_manager import block_manager
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

config: MutableMapping[str, Any] = {}
web_address: str = ""


@app.on_event("startup")
def startup():
    """When the application starts read the config
    """
    global config, web_address

    config = load_config("../data/uaasr.toml")
    web_address = config["web_interface"]["address"]

    # transaction_analyser.set_config(config)
    # load_and_process_blocks()


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
    """ Return the current service status (should be `Ready`)"""
    return logic.get_status()


@app.get("/addr", tags=["Addresses"])
def get_addr() -> Dict[str, Any]:
    """ Return the peer addresses seen by the service"""

    return address_manager.get_peers()


@app.get("/tx", tags=["Tx"])
def get_tx(hash: str) -> Dict[str, Any]:
    """ Return the tx entry identified by hash"""
    return tx_analyser.get_tx_entry(hash)


@app.get("/tx/mempool", tags=["Tx"])
def get_mempool() -> Dict[str, Any]:
    """ Return the mempool seen by the service"""
    return tx_analyser.get_mempool()


@app.get("/tx/utxo", tags=["Tx"])
def get_utxo(hash: str) -> Dict[str, Any]:
    """ Return the utxo entry identified by hash"""
    return tx_analyser.get_utxo_entry(hash)


@app.get("/block/latest", tags=["Block"])
def get_latest_blocks() -> Dict[str, Any]:
    """ Return the latest blocks seen by the service"""
    return block_manager.get_latest_blocks()


@app.get("/block/height", tags=["Block"])
def get_block_at_height(height: int) -> Dict[str, Any]:
    """ Return the block at the given height"""
    return block_manager.get_block(height)
