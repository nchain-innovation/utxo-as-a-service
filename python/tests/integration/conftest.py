import os
import sys

INTEGRATION_DIR = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, INTEGRATION_DIR)

import pytest
from fastapi.testclient import TestClient

from helpers import (
    build_integration_config,
    clear_blocks,
    init_integration_schema,
)

pytestmark = pytest.mark.integration


@pytest.fixture(scope="session")
def mysql_url() -> str:
    url = os.environ.get("UAAS_TEST_MYSQL_URL")
    if not url:
        pytest.skip("UAAS_TEST_MYSQL_URL not set")
    return url


@pytest.fixture(scope="session")
def integration_config(mysql_url: str):
    init_integration_schema(mysql_url)
    return build_integration_config(mysql_url)


@pytest.fixture(scope="session")
def configured_services(integration_config):
    from blockfile import blockfile
    from collection import collection
    from database import database
    from logic import logic
    from tx_analyser import tx_analyser

    database.set_config(integration_config)
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
