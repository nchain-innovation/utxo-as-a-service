# UTXO as a Service - Configuration

Configuration for this service can be found in the `data\uaas.toml` toml file.
The toml file is read when the service starts.
The toml configuration file is also used by the Python REST interface.

This document describes each element of the configuration file in the order that they are presented in the file.

## Service Section

```toml
[service]
user_agent = "/Bitcoin SV:1.0.9/"
network = "testnet"
```

The `service section contains the following:
* `user_agent` which provides the string that the service presents to the peer node on the network.
* `network` which identifies the blockchain network that the service connects to, either `testnet` or `mainnet`.

The `network` field determines the section of the configuration file that is read for the network settings (see next section).

## Network Settings
There are two network settings sections `testnet` and `mainnet`, the `network` field in the service section determines which section is read.

This enables the service to be switched between the two networks without changing any of the settings for the other network.

```toml
[testnet]
ip = [ "176.9.148.163"]
port = 18333

start_block_hash = "000000000001f6f089b463c84c6509707db324f6f8e0c05324e856282c8b33d8"
start_block_height = 1485944

timeout_period = 240.0
startup_load_from_database = true

# Python database access
host = "host.docker.internal"
user = "uaas"
password = "uaas-password"
database = "uaas_db"

block_file = "../data/block.dat"
save_blocks = true
```

The Network setting section contains the following fields:
* `ip` -  a list of the ip addresses of BSV nodes that the service will connect to
* `port` - the port that the service will connect to on the BSV node. This is typically set to `8333` for mainnet and `18333` for testnet
* `start_block_hash` - identifies the first block that the service should work from the blockchain network. This allows the service to operate from a particular block rather that having to download all blocks since thes genesis block
* `start_block_height` - this is the heigh of the `start_block`. This ensures that the REST API can return the correct block for a given block height
* `timeout_period` - the time thee service will wait without receiving messages from a peer before declaring the connection `timed out`
* `startup_load_from_database` - makes the service load the data from the database on startup, this is the normal operation.

If this is set to `false` the service will load from the block file (see later), this is useful if the database structure is changed and we and want to repopulate the data without having to redownload all the blocks.
Note when reading from the file, would expect to delete the following tables: blocks, tx, utxo, mempool, Prior to starting the service.

* `block_file` - identifies where the blocks are stored, used by both the Rust service and Python REST API
* `save_blocks` - when true the Rust service saves blocks to the `block_file`, when false no blocks are saved.


### Python database access used by the Python REST API
* `host` - the database host
* `user` - the database user
* `password` - the database password
* `database` - the database connection

## Database
Information used to configure the Rust database connection
```toml
[database]

mysql_url = "mysql://uaas:uaas-password@localhost:3306/uaas_db"
mysql_url_docker = "mysql://uaas:uaas-password@host.docker.internal:3306/uaas_db"

ms_delay = 300
retries = 6
```

* `mysql_url` - this is the url of the database, this is used by the Rust service on the local machine
* `mysql_url_docker` - as `mysql_url` but for use in a Docker container
* `ms_delay` - if a datase connection fails, this is the delay before retrying in milliseconds.
* `retries` - this is the number of times to retry a database connection before declaring the connection broken.


## Orphan Detection
Settings regarding detecting orphan blocks.
```toml
[orphan]
detect = true
threshold = 100
```
* `detect` - when set to `true` the service will look for orphan blocks. The service does not detect orphan blocks. However we have seen that when the hash of an unknown block is used as last known, header when requesting blocks, this results in the peer sending blocks from 2011. Typically prior to the first block the service has been configured to receive. When this happens the service will copy the block header to the `orphan` table and remove the block from the `blocks` table.
* `threshold` - is the number of blocks before we start looking for orphan blocks

## Collections
Collections are used to identify transactions that are of interest. The service can follow multiple Collections.
Note that each collection is defined in double square brackets `[[]]`.
The following collection captures all Pay to Public Key Hash (P2PKH) transactions.

```toml
[[collection]]
name = "p2pkh"
locking_script_pattern = "76a914[0-9a-f]{40}88ac"
track_descendants = false
```
Each collection section has  the following fields:
* `name` - the name of the collection, the service will create a table with this name and store collection matching transaction in it
* `locking_script_pattern` - a regular expression that identifies the locking script that defines the transactions of interest
* `track_descendants` - a flag to indicate if decendent transactions should also be captured.


## REST API Web Interface
This section identifies the address and port of the REST API.

```toml
[web_interface]
address = '127.0.0.1:5010'
log_level = 'info'
reload = false
```
In the example above the REST API will be provided at http://127.0.0.1:5010/docs as a Swagger interface that the user can interact with.

The web interface section has the following fields:
* `address` - this is the address and port that REST API will be provided on
* `log_level` - this is the level that the REST API logs events at
* `reload` - if set to true the webserver will reload if the source code is changed

