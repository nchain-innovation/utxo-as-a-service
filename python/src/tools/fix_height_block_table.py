#!/usr/bin/python3

import sys
sys.path.append('..')

from database import database
from util import load_config, ConfigType


def get_max_height(table_name: str) -> int:
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
    max_height = get_max_height(table_name)
    print(f"max_height = {max_height}")

    for i in range(7050, max_height + 1):
        query = f"UPDATE {table_name} SET height={i + start_height + 1} WHERE height='{i}';"
        r = database.query(query)
        print(r)


if __name__ == '__main__':
    main()
