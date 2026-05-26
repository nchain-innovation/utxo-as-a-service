#!/usr/bin/python3

import sys
sys.path.append('..')

from database import database
from config import load_config, ConfigType

ALLOWED_TABLES = frozenset({"blocks", "utxo", "tx"})


def get_max_height(table_name: str) -> int:
    if table_name not in ALLOWED_TABLES:
        raise ValueError(f"Invalid table name: {table_name}")
    retval = database.query(f"SELECT max(height) FROM {table_name};")
    result = list(map(lambda x: x[0], retval))[0]
    return int(result)


def get_start_block_height(config: ConfigType) -> int:
    network = config["service"]["network"]
    return config[network]["start_block_height"]


def main():
    """ This corrected the block table to have a height the same as the blockchain
    """

    config = load_config("../../data/uaasr.toml")
    database.set_config(config)

    start_height = get_start_block_height(config)
    print(f"start_height = {start_height}")

    # table_name = "blocks"
    # table_name = "utxo"
    table_name = "tx"
    if table_name not in ALLOWED_TABLES:
        raise ValueError(f"Invalid table name: {table_name}")
    max_height = get_max_height(table_name)
    print(f"max_height = {max_height}")

    for i in range(7050, max_height + 1):
        query = f"UPDATE {table_name} SET height = %s WHERE height = %s;"
        r = database.query(query, (i + start_height + 1, i))
        print(r)


if __name__ == '__main__':
    main()
