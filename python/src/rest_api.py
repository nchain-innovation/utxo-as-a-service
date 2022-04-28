from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from typing import Any, MutableMapping, Dict

from util import load_config
from address_manager import address_manager


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
    # Placeholder for now
    service_status = "Ready"
    return {
        'status': service_status,
        'network': config['service']['network'],
        'number_of_blocks': 0,
        'number_of_tx': 0,
        'number_of_utxo': 0,
    }


@app.get("/addr", tags=["Status"])
def get_addr() -> Dict[str, Any]:
    """ Return the peer addresses seen by the service"""

    return address_manager.get_peers()
