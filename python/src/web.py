#!/usr/bin/python3
import logging

import uvicorn

from database import database
from blockfile import blockfile
from tx_analyser import tx_analyser
from logic import logic

from config import load_config_or_exit, ConfigType
from collection import collection

LOGGER = logging.getLogger(__name__)

LOCAL_HOSTS = frozenset({"127.0.0.1", "localhost", "::1"})


def run_webserver(config: ConfigType):
    """ Given the config run the webserver
    """
    address = config["address"]
    (host, port) = address.split(":")

    # Warn loudly if the API is reachable off-host without authentication:
    # the broadcast and collection-monitor endpoints mutate state.
    if not config.get("api_key") and host not in LOCAL_HOSTS:
        LOGGER.warning(
            "SECURITY: binding to %s with no api_key set - tx broadcast and "
            "monitor endpoints will be UNAUTHENTICATED. Set [web_interface].api_key "
            "or bind to 127.0.0.1 behind a reverse proxy.",
            host,
        )
    LOGGER.info("host is set to: %s", host)

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
    config = load_config_or_exit("../data/uaasr.toml")
    database.set_config(config)
    blockfile.set_config(config)
    tx_analyser.set_config(config)
    logic.set_config(config)
    collection.set_config(config)
    run_webserver(config["web_interface"])


if __name__ == "__main__":
    main()
