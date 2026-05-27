from typing import Any, MutableMapping
from urllib.parse import urlparse

from config import ConfigType, _validate_config


def parse_mysql_url(url: str) -> dict[str, Any]:
    parsed = urlparse(url)
    if parsed.scheme != "mysql" or not parsed.hostname or not parsed.path:
        raise ValueError(f"Invalid MySQL URL: {url}")
    return {
        "host": parsed.hostname,
        "port": parsed.port or 3306,
        "user": parsed.username or "",
        "password": parsed.password or "",
        "database": parsed.path.lstrip("/"),
    }


def build_integration_config(mysql_url: str) -> ConfigType:
    db = parse_mysql_url(mysql_url)
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
            "host": db["host"],
            "user": db["user"],
            "password": db["password"],
            "database": db["database"],
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
            "host": db["host"],
            "user": db["user"],
            "password": db["password"],
            "database": db["database"],
            "block_file": "../data/test-net.dat",
            "save_blocks": False,
            "save_txs": False,
        },
        "database": {
            "mysql_url": mysql_url,
            "mysql_url_docker": mysql_url,
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


def init_integration_schema(mysql_url: str) -> None:
    import mysql.connector

    db = parse_mysql_url(mysql_url)
    connection = mysql.connector.connect(
        host=db["host"],
        port=db["port"],
        user=db["user"],
        password=db["password"],
        database=db["database"],
    )
    cursor = connection.cursor()
    try:
        cursor.execute(
            """
            CREATE TABLE IF NOT EXISTS blocks (
                height int unsigned not null,
                hash varchar(64) not null,
                version int unsigned not null,
                prev_hash varchar(64) not null,
                merkle_root varchar(64) not null,
                timestamp int unsigned not null,
                bits int unsigned not null,
                nonce int unsigned not null,
                `offset` bigint unsigned not null,
                blocksize int unsigned not null,
                numtxs int unsigned not null,
                PRIMARY KEY (hash)
            )
            """
        )
        connection.commit()
    finally:
        cursor.close()
        connection.close()


def clear_blocks(mysql_url: str) -> None:
    import mysql.connector

    db = parse_mysql_url(mysql_url)
    connection = mysql.connector.connect(
        host=db["host"],
        port=db["port"],
        user=db["user"],
        password=db["password"],
        database=db["database"],
    )
    cursor = connection.cursor()
    try:
        cursor.execute("DELETE FROM blocks")
        connection.commit()
    finally:
        cursor.close()
        connection.close()


def insert_sample_block(mysql_url: str, height: int, block_hash: str) -> None:
    import mysql.connector

    db = parse_mysql_url(mysql_url)
    connection = mysql.connector.connect(
        host=db["host"],
        port=db["port"],
        user=db["user"],
        password=db["password"],
        database=db["database"],
    )
    cursor = connection.cursor()
    try:
        cursor.execute(
            """
            INSERT INTO blocks
            (height, hash, version, prev_hash, merkle_root, timestamp, bits, nonce,
             `offset`, blocksize, numtxs)
            VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
            """,
            (
                height,
                block_hash,
                1,
                "b" * 64,
                "c" * 64,
                1_700_000_000,
                0x1D00FFFF,
                0,
                0,
                1000,
                1,
            ),
        )
        connection.commit()
    finally:
        cursor.close()
        connection.close()
