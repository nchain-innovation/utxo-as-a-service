from io import BytesIO
from typing import Any, MutableMapping
from p2p_framework.object import CBlock


# Blockheaders
def load_block_at_offset(fname: str, offset: int) -> CBlock:
    """ Load the block at an offset in a file
    """
    block = CBlock()
    try:
        with open(fname, "rb") as fh:
            fh.seek(offset)
            f = BytesIO(fh.read())
            block.deserialize(f)
    except FileNotFoundError as e:
        print(f"load_blocks - File not found: {e}")
    else:
        # Calculate the hashes
        block.rehash()
        list(map(lambda x: x.rehash(), block.vtx))
    return block


class BlockFile:
    """ Used to provide an interface to the block file
    """
    def __init__(self):
        self.block_file: str

    def set_config(self, config: MutableMapping[str, Any]):
        network = config['service']['network']
        self.block_file = config[network]["block_file"]

    def load_at_offset(self, offset: int) -> CBlock:
        return load_block_at_offset(self.block_file, offset)


blockfile = BlockFile()
