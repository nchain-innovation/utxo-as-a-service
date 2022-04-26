BIP0031_VERSION = 60000
MY_VERSION = 70015  # INVALID_CB_NO_BAN_VERSION
# MY_SUBVERSION = b"/python-mininode-tester:0.0.3/"
MY_SUBVERSION = b"/Bitcoin SV:1.0.7/"
# from version 70001 onwards, fRelay should be appended to version messages (BIP37)
MY_RELAY = 1

MAX_INV_SZ = 50000
MAX_PROTOCOL_RECV_PAYLOAD_LENGTH = 2 * 1024 * 1024
LEGACY_MAX_PROTOCOL_PAYLOAD_LENGTH = 1 * 1024 * 1024

COIN = 100000000  # 1 btc in satoshis

NODE_NETWORK = (1 << 0)
NODE_GETUTXO = (1 << 1)  # BIP 64
NODE_BLOOM = (1 << 2)
NODE_WITNESS = (1 << 3)
NODE_XTHIN = (1 << 4)
NODE_BITCOIN_CASH = (1 << 5)

# Howmuch data will be read from the network at once
READ_BUFFER_SIZE = 8192


# ports used by chain type
NETWORK_PORTS = {
    "mainnet": 8333,
    "testnet3": 18333,
    "stn": 9333,
    "regtest": 18444
}
