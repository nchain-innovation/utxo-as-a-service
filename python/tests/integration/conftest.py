import os
import sys

INTEGRATION_DIR = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, INTEGRATION_DIR)

import pytest
from fastapi.testclient import TestClient

from helpers import (
    build_integration_config,
    clear_blocks,
    clear_utxo,
    init_integration_schema,
    prepare_mysql_for_tests,
)

pytestmark = pytest.mark.integration


@pytest.fixture(scope="session")
def mysql_url() -> str:
    import mysql.connector

    url = os.environ.get("UAAS_TEST_MYSQL_URL")
    if not url:
        pytest.skip("UAAS_TEST_MYSQL_URL not set")
    try:
        prepare_mysql_for_tests(url)
    except mysql.connector.Error as err:
        pytest.skip(
            "MariaDB unavailable for integration tests "
            f"({err}). Ensure docker-compose database is running and "
            "UAAS_TEST_MYSQL_URL credentials are valid."
        )
    return url


@pytest.fixture(scope="session")
def integration_config(mysql_url: str):
    init_integration_schema(mysql_url)
    return build_integration_config(mysql_url)


@pytest.fixture(scope="session")
def configured_services(integration_config):
    import mysql.connector

    from blockfile import blockfile
    from collection import collection
    from database import database
    from logic import logic
    from tx_analyser import tx_analyser

    try:
        database.set_config(integration_config)
    except mysql.connector.Error as err:
        pytest.skip(f"MariaDB connection pool setup failed: {err}")
    blockfile.set_config(integration_config)
    tx_analyser.set_config(integration_config)
    logic.set_config(integration_config)
    collection.set_config(integration_config)
    return integration_config


@pytest.fixture
def client(configured_services) -> TestClient:
    import rest_api

    return TestClient(rest_api.app)


@pytest.fixture
def clean_blocks(mysql_url: str):
    clear_blocks(mysql_url)
    yield
    clear_blocks(mysql_url)


@pytest.fixture
def clean_utxo(mysql_url: str):
    clear_utxo(mysql_url)
    yield
    clear_utxo(mysql_url)
