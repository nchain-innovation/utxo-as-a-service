#!/usr/bin/python3

from typing import MutableMapping, Any
import uvicorn
import os

from database import database
from blockfile import blockfile
from logic import logic

from util import load_config
from collection import collection


def run_webserver(config: MutableMapping[str, Any]):
    """ Given the config run the webserver
    """
    address = config["address"]
    (host, port) = address.split(":")

    if os.environ.get("APP_ENV") == "docker":
        print("Running in Docker")
        # Allow all access in docker
        # (required as otherwise the localmachine can not access the webserver)
        # set host in the toml file to: 0.0.0.0
    else:
        print("Running in native OS")
        # Only allow access from localmachine
        # set host in the toml file to 127.0.0.1
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
