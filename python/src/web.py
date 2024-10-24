#!/usr/bin/python3

from typing import MutableMapping, Any
import uvicorn

from database import database
from blockfile import blockfile
from logic import logic

from config import load_config
from collection import collection


def run_webserver(config: MutableMapping[str, Any]):
    """ Given the config run the webserver
    """
    address = config["address"]
    (host, port) = address.split(":")
    print(f"host is set to: {host}")

    # Run as HTTP
    uvicorn.run(
        "rest_api:app",
        host=host,
        port=int(port),
        log_level=config["log_level"],
        reload=config["reload"],
        workers=1,  # Don't change this number unless you understand the full implications of having shared data.
    )


def main():
    """ main function - reads config, sets up system starts REST API
    """
    config = load_config("../data/uaasr.toml")
    database.set_config(config)
    blockfile.set_config(config)
    logic.set_config(config)
    collection.set_config(config)
    run_webserver(config["web_interface"])


if __name__ == "__main__":
    main()
