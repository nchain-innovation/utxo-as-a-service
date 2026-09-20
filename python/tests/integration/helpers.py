"""Fixtures for the integration suite.

**These helpers delete rows.** `clear_blocks` and `clear_utxo` issue
unconditional DELETEs, so this suite must never be pointed at a database
anything cares about. `guard_test_database` enforces that rather than trusting
the reader: it refuses a database whose name does not look like a test one.

No schema is defined here any more. It used to carry its own CREATE TABLE for
blocks, tx, utxo and mempool — a fourth definition of the schema that had
already drifted from the others. `rust/migrations/` owns the schema and
`uaas migrate` applies it; this only checks that it has been.
"""

from typing import Any
from urllib.parse import urlparse

import psycopg
import pytest

from config import ConfigType, _validate_config
from hashes import identifier_to_bytes, txid_to_bytes

# Tables the suite reads or writes. Checked up front so a missing schema says
# so once, rather than surfacing as an unrelated failure in each test.
REQUIRED_TABLES = ("blocks", "tx", "utxo", "utxo_spent", "mempool", "collection")

# Opt-out for someone who genuinely means to run this against a database whose
# name does not look like a test one. Deliberately awkward.
_OVERRIDE_ENV = "UAAS_TEST_ALLOW_DESTRUCTIVE"


def connect(url: str) -> psycopg.Connection:
    return psycopg.connect(url)


def guard_test_database(url: str) -> None:
    """Refuse a database this suite should not be deleting rows from.

    The suite truncates tables. Pointed at a running deployment it would
    destroy indexed chain data, and the connection string differs from the
    production one by a few characters. A name check is crude but it catches
    the mistake that actually happens: reusing the URL from docker-compose.
    """
    import os

    if os.environ.get(_OVERRIDE_ENV):
        return
    name = urlparse(url).path.lstrip("/")
    if "test" not in name.lower():
        pytest.skip(
            f"refusing to run destructive integration tests against database "
            f"'{name}': its name does not contain 'test'. Point "
            f"UAAS_TEST_POSTGRES_URL at a throwaway database, or set "
            f"{_OVERRIDE_ENV}=1 if you really mean it."
        )


def verify_schema(url: str) -> None:
    """Fail, with something actionable, if the migrations have not been run.

    Reachable-but-unmigrated is a setup mistake, not a reason to skip: skipping
    looks identical to passing.
    """
    with connect(url) as connection:
        with connection.cursor() as cursor:
            cursor.execute(
                "SELECT table_name FROM information_schema.tables "
                "WHERE table_schema = current_schema()"
            )
            present = {row[0] for row in cursor.fetchall()}
    missing = [t for t in REQUIRED_TABLES if t not in present]
    if missing:
        raise AssertionError(
            f"the test database is missing {', '.join(missing)}. "
            "Run `cargo run -- migrate \"$UAAS_TEST_POSTGRES_URL\"` from rust/ "
            "first: the schema lives in rust/migrations/ and is not created by "
            "the tests."
        )


def build_integration_config(url: str) -> ConfigType:
    config: ConfigType = {
        "service": {
            "user_agent": "/Bitcoin SV:1.0.11/",
            "network": "testnet",
            "rust_address": "127.0.0.1:8081",
        },
        "mainnet": {
            "ip": ["127.0.0.1"],
            "port": 8333,
            "start_block_hash": "0" * 64,
            "start_block_height": 1,
            "timeout_period": 240.0,
            "startup_load_from_database": False,
            "block_file": "../data/main-block.dat",
            "save_blocks": False,
            "save_txs": False,
        },
        "testnet": {
            "ip": ["127.0.0.1"],
            "port": 18333,
            "start_block_hash": "0" * 64,
            "start_block_height": 1,
            "timeout_period": 240.0,
            "startup_load_from_database": False,
            "block_file": "../data/test-net.dat",
            "save_blocks": False,
            "save_txs": False,
        },
        # The per-network host/user/password/database/mysql_port keys are gone.
        # Both components read these two, chosen by APP_ENV. Same value here:
        # the tests run against one database either way.
        "database": {
            "postgres_url": url,
            "postgres_url_docker": url,
            "ms_delay": 300,
            "retries": 3,
        },
        "orphan": {"detect": False, "threshold": 100},
        "logging": {"level": "info"},
        "utxo": {"complete": 6},
        "dynamic_config": {"filename": "../data/dynamic.toml"},
        "collection": [],
        "web_interface": {
            "address": "127.0.0.1:5010",
            "log_level": "info",
            "reload": False,
            "rust_url": "http://127.0.0.1:8081",
        },
    }
    _validate_config(config, "integration-test")
    return config


def _execute(url: str, statements: list[tuple[str, tuple[Any, ...]]]) -> None:
    with connect(url) as connection:
        with connection.cursor() as cursor:
            for sql, params in statements:
                cursor.execute(sql, params)


def fetch(url: str, sql: str, params: tuple[Any, ...] = ()) -> list[tuple[Any, ...]]:
    """Run one statement and return its rows, for tests that need to look at
    the database directly rather than through the service layer."""
    with connect(url) as connection:
        with connection.cursor() as cursor:
            cursor.execute(sql, params)
            if cursor.description is None:
                return []
            return list(cursor.fetchall())


def clear_blocks(url: str) -> None:
    # tx first: it is read via a join on blocks.height, and leaving orphaned
    # rows behind makes the next test's failure hard to read.
    _execute(url, [("DELETE FROM tx", ()), ("DELETE FROM blocks", ())])


def clear_utxo(url: str) -> None:
    _execute(
        url,
        [
            ("DELETE FROM utxo_monitor", ()),
            ("DELETE FROM utxo_spent", ()),
            ("DELETE FROM utxo", ()),
        ],
    )


def insert_sample_block(url: str, height: int, block_hash: str) -> None:
    """A block row, from a txid as the API states it.

    The hash columns are bytea in internal order, so the display hex the tests
    pass has to be reversed on the way in — exactly as the service does it.
    """
    _execute(
        url,
        [(
            """
            INSERT INTO blocks
            (height, hash, version, prev_hash, merkle_root, block_time, bits,
             nonce, file_offset, blocksize, numtxs)
            VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
            """,
            (
                height,
                txid_to_bytes(block_hash),
                1,
                txid_to_bytes("b" * 64),
                txid_to_bytes("c" * 64),
                1_700_000_000,
                0x1D00FFFF,
                0,
                0,
                1000,
                1,
            ),
        )],
    )


def insert_sample_utxo(
    url: str,
    tx_hash: str,
    pubkeyhash: str,
    height: int,
    satoshis: int,
    pos: int = 0,
) -> None:
    """A live utxo row.

    `pubkeyhash` populates `identifier`, which is what replaced it: for the
    p2pkh pattern the captured bytes are the pubkeyhash. It is not reversed —
    an identifier comes from the locking script as written.

    `height` of -1 means "not in a block", which is NULL now rather than a
    sentinel. `locking_script` is NOT NULL, so a minimal p2pkh prefix stands in.
    """
    _execute(
        url,
        [(
            """
            INSERT INTO utxo
            (txid, vout, satoshis, locking_script, identifier, created_height)
            VALUES (%s, %s, %s, %s, %s, %s)
            """,
            (
                txid_to_bytes(tx_hash),
                pos,
                satoshis,
                b"\x76\xa9\x14",
                identifier_to_bytes(pubkeyhash),
                None if height < 0 else height,
            ),
        )],
    )
