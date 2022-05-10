# UTXO as a Service - Configuration

Configuration for this service can be found in the `data\uaas.toml` toml file.


NOTE THIS DOCUMENT IS STILL DRAFT!!!!

## Network
This identifies the blockchain network that the service connects to, either `testnet3` or `mainnet`.
```toml
[Network]
network = "testnet3"
```
## Blockchain
This identifies the first block that the service should look for on the current blockchain network.
```toml
[Blockchain]
start_block_hash = "0000000017fe8005708d875d1f61e11075701629420c8eab4d04e50eb98c5872"
```
## Peer Addresses
The `addresses` is a list of peer BSV mining nodes to connect to on the current blockchain network.

```toml
[Peers]
port = 18333
addresses = [
    "176.9.148.163",
    "167.99.91.85",
    "138.68.156.46",
    "206.189.203.168",
]
```
The `addr_file` configures where the service saves peer addresses that it has been notified of by peers.
```toml
[Addrs]
addr_file = "data/addr.dat"
```

## Blocks
This identifies where the blocks are stored.


```toml
[Blocks]
block_file = "data/block.dat"
```


## Transactions
This identifies if the transactions are pruned and where the transactions are stored.
### Transaction Prune
* If set to `false` the system will keep all blocks.
* If set to `true` the system will only keep transactions of interest, including:
    * UTXO
    * Transactions identified by Collections

```toml
[Transactions]
prune = false
state_file = "data/tx_state_testnet.dat"

```
## Logging
This configures where the service logs are written to and the level of the logging.
```toml
[Logging]
log_file = "data/logs.txt"
level = "INFO"
```

## Collections
Collections are used to identify transactions that are of interest. The service can follow multiple Collections.

Each collection has a name and is identified by regular expression pattern in either `script_sig` or `script_pub_key`.

If `track_descendants = true` then the decendant transactions are also captured.

The following collection captures all tranactions associated with the address 'mnoCNkL8GeBaFmFbsaaZQVxY1vqTaw83nG'
```toml
[[Collection]]

# These fields are required
name = "address mnoCNkL8GeBaFmFbsaaZQVxY1vqTaw83nG"
track_descendants = false
# These fields are optional (script_sig_pattern and script_pub_key_pattern)
# script_sig_pattern = ""
script_pub_key_pattern = "76a9144fdb4a40372ff0fd550a57b4e3ce3fab95ad680688ac"
```

## Web Interface
This identifies the address and port of the REST API. In this case http://127.0.0.1:5010/docs will provide a Swagger interface that the user can interact with.
```toml
[web_interface]
address = '127.0.0.1:5010'
log_level = 'info'
reload = false
```
