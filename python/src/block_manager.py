from typing import List, Dict, Any, MutableMapping
import datetime

from mysql.connector import connect

from util import load_block_at_offset


class BlockManager:
    def __init__(self):
        self.host: str
        self.user: str
        self.password: str
        self.database: str
        self.block_file: str

    def set_config(self, config: MutableMapping[str, Any]):
        self.host = config["python"]["host"]
        self.user = config["python"]["user"]
        self.password = config["python"]["password"]
        self.database = config["python"]["database"]
        self.block_file = config["shared"]["block_file"]

    def _read_latest_blocks(self) -> List[Dict[str, Any]]:
        # Read blocks from database
        with connect(
            host=self.host,
            user=self.user,
            password=self.password,
            database=self.database,
        ) as connection:
            query = ("SELECT * FROM blocks ORDER BY height desc LIMIT 20;")
            cursor = connection.cursor()
            cursor.execute(query)
            retval = []
            for x in cursor:
                timestamp = datetime.datetime.fromtimestamp(x[5])

                retval.append({
                    "height": x[0], "hash": x[1], "version": x[2], "prev_hash": x[3], "merkle_root": x[4],
                    "timestamp": timestamp.strftime('%Y-%m-%d %H:%M:%S'),
                    "bits": x[6], "nonce": x[7], "offset": x[8]
                })
            return retval

    def get_latest_blocks(self) -> Dict[str, List[Dict[str, Any]]]:
        """ Return a dictionary of blocks"""
        return {
            "blocks": self._read_latest_blocks(),
        }

    def _read_block(self, height: int) -> List[Dict[str, Any]]:
        # Read block from database
        with connect(
            host=self.host,
            user=self.user,
            password=self.password,
            database=self.database,
        ) as connection:
            query = (f"SELECT * FROM blocks WHERE height = '{height}';")
            cursor = connection.cursor()
            cursor.execute(query)
            retval = []
            for x in cursor:
                timestamp = datetime.datetime.fromtimestamp(x[5])
                retval.append({
                    "height": x[0], "hash": x[1], "version": x[2], "prev_hash": x[3], "merkle_root": x[4],
                    "timestamp": timestamp.strftime('%Y-%m-%d %H:%M:%S'),
                    "bits": x[6], "nonce": x[7], "offset": x[8]
                })
            return retval

    def _read_block_offset(self, height: int) -> int:
        # Read block from database
        with connect(
            host=self.host,
            user=self.user,
            password=self.password,
            database=self.database,
        ) as connection:
            query = (f"SELECT offset FROM blocks WHERE height = '{height}';")
            cursor = connection.cursor()
            cursor.execute(query)
            retval = []
            for x in cursor:
                retval.append(x)
                print(f"x = {x}")

            return retval[0][0]
            """
            for x in cursor:
                timestamp = datetime.datetime.fromtimestamp(x[5])
                retval.append({
                    "height": x[0], "hash": x[1], "version": x[2], "prev_hash": x[3], "merkle_root": x[4],
                    "timestamp": timestamp.strftime('%Y-%m-%d %H:%M:%S'),
                    "bits": x[6], "nonce": x[7], "offset": x[8]
                })
            return retval
            """

    def get_block(self, height: int) -> Dict[str, Dict[str, Any]]:
        # Return the block at the given height
        offset = self._read_block_offset(height)
        block = load_block_at_offset(self.block_file, offset)
        return {
            # "block": self._read_block(height),
            "block": block.to_dict(),
        }


block_manager = BlockManager()


def test_database():
    config = {
        "python":
            {
                "host": "host.docker.internal",
                "user": "uaas",
                "password": "uaas-password",
                "database": "uaas_db",
            }
    }

    block_manager.set_config(config)
    retval = block_manager.get_latest_blocks()
    print(f"retval = {retval}")


if __name__ == '__main__':
    test_database()