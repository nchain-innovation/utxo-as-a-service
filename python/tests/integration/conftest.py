import os
import sys

INTEGRATION_DIR = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, INTEGRATION_DIR)

import psycopg  # noqa: E402
import pytest  # noqa: E402
from fastapi.testclient import TestClient  # noqa: E402

from helpers import (  # noqa: E402
    build_integration_config,
    clear_blocks,
    clear_utxo,
    guard_test_database,
    verify_schema,
)

pytestmark = pytest.mark.integration


@pytest.fixture(scope="session")
def postgres_url() -> str:
    url = os.environ.get("UAAS_TEST_POSTGRES_URL")
    if not url:
        pytest.skip("UAAS_TEST_POSTGRES_URL not set")

    # Before anything connects: this suite deletes rows.
    guard_test_database(url)

    try:
        with psycopg.connect(url):
            pass
    except psycopg.OperationalError as err:
        pytest.skip(
            f"PostgreSQL unavailable for integration tests ({err}). Start one "
            "and check UAAS_TEST_POSTGRES_URL."
        )

    # Unreachable is a skip; reachable but unmigrated is a failure, because a
    # skip there is indistinguishable from a pass.
    verify_schema(url)
    return url


@pytest.fixture(scope="session")
def integration_config(postgres_url: str):
    return build_integration_config(postgres_url)


@pytest.fixture(scope="session")
def configured_services(integration_config):
    from blockfile import blockfile
    from collection import collection
    from database import database
    from logic import logic
    from tx_analyser import tx_analyser

    try:
        database.set_config(integration_config)
    except psycopg.OperationalError as err:
        pytest.skip(f"PostgreSQL connection pool setup failed: {err}")
    blockfile.set_config(integration_config)
    tx_analyser.set_config(integration_config)
    logic.set_config(integration_config)
    collection.set_config(integration_config)
    yield integration_config
    # The pool runs background threads; leaving it to be collected at
    # interpreter shutdown raises PythonFinalizationError from its finaliser.
    database.close()


@pytest.fixture
def client(configured_services) -> TestClient:
    import rest_api

    return TestClient(rest_api.app)


@pytest.fixture
def clean_blocks(postgres_url: str):
    clear_blocks(postgres_url)
    yield
    clear_blocks(postgres_url)


@pytest.fixture
def clean_utxo(postgres_url: str):
    clear_utxo(postgres_url)
    yield
    clear_utxo(postgres_url)
