# UTXO as a Service - Configuration

Configuration for this service can be found in the `data/uaasr.toml` file.
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

postgres_url = "postgresql://uaas:CHANGE-ME@localhost:5433/uaas_db"
postgres_url_docker = "postgresql://uaas:CHANGE-ME@host.docker.internal:5433/uaas_db"

ms_delay = 300
retries = 6
```

The two URLs are placeholders in the tracked config: this repository is public,
so a working one must not be committed. `UAAS_POSTGRES_URL` overrides both and
is what `docker-compose.yml` sets; the service refuses to start if neither it
nor an untracked config supplies a real URL. See
[Security.md](Security.md#credentials).

* `postgres_url` - libpq connection URL for the database, used by the Rust service on the local machine
* `postgres_url_docker` - as `postgres_url` but for use in a Docker container, where the host is the compose service name and the port is the container's own, not the published one
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


## Mempool eviction
When the service gives up on a spend that was broadcast and never mined.
```toml
[mempool]
eviction_blocks = 144
```
* `eviction_blocks` - the number of blocks an unconfirmed spend may go unmined
  before its outpoint is returned to the spendable set and its `mempool` row is
  removed. Set to `0` to disable eviction entirely.

Without this, a spend that never confirms leaves its outpoint in neither the
spendable set nor a settled spend, so the UTXO set is under-reported
permanently and nothing reports it. A fee too low to be mined, or the losing
side of a double-spend, is enough to cause it.

The threshold is a policy choice with a cost on both sides. Too low and a slow
but valid spend is briefly counted as spendable while it is still in flight;
too high and a dropped spend under-reports the set for longer. The default of
144 is roughly a day at ten-minute blocks, on the basis that a transaction
unmined for a day is not going to be mined.

Age is counted in **blocks, not elapsed time**: wall-clock age keeps advancing
while the service is stopped, so after an outage every pending spend would look
ancient at once. See [Database.md](Database.md) for how the state is stored.

Eviction counts are logged at `warn`, which is the level to alert on. They are
not logged at `info` because `info` is compiled out of release builds.

# Logging
This sets the logging messages level produced by the Rust service.
```toml
[logging]
level = "info"
```
* `level` is the logging level.

The logging level can be one of:
* `"error"` - Designates very serious errors.
* `"warn"` - Designates hazardous situations.
* `"info"` - Designates useful information.
* `"debug"` - Designates useful information.
* `"trace"` - Designates very low priority, often extremely verbose, information.


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
* `require` - which property a locking script must have before the collection selects it. Optional; omitted, it is `bytes_present`, which is the behaviour every collection had before this field existed.

### `require`

| Value | Meaning |
|-------|---------|
| `bytes_present` (default) | The pattern's bytes appear somewhere in the locking script. |
| `signature_operand` | The match covers whole opcodes — or exactly one push element — on a path that executes, **and** the element it selects reaches a signature check. |

`bytes_present` establishes less than it looks. Anyone can put any bytes in any
output for the cost of a dust payment, so a match says the bytes are there, not
that the output pays the monitored key. Consumers were reading it as the
second.

`signature_operand` asks the stricter question. It is **opt-in per collection**
because it changes what a collection captures, and because it is wrong for some
of them:

* **Data-protocol collections must keep the default.** `dsa` and `CoCv1` match
  `OP_RETURN` payloads, which are data by definition and can never be an
  operand to anything.
* **`1sat` must keep the default.** A 1Sat-Ordinal inscription envelope lives
  in a branch that never executes and is not an operand to a signature check,
  so requiring the strict property would empty that collection rather than
  tighten it.
* **Address-derived and key monitors are what it is for** — the case where
  "this output pays that key" is what the monitor means.

The analysis is deliberately conservative: anything it cannot model answers
"no" rather than "probably", because a false positive is the failure this
exists to remove. Branches are the clearest case — whether one executes depends
on the unlocking script, which an output does not carry — so a pattern matching
inside an `OP_IF` will not satisfy `signature_operand`.

A pattern that embeds the push opcode, such as `21<key>`, is encoding-specific
by construction: the same key pushed with `OP_PUSHDATA1` will not match it.
Write the pattern against the element alone if the collection should be
independent of how the element was pushed.


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
* `rust_url` - base URL of the Rust backend used for broadcast and collection monitor operations
* `api_key` - *(optional)* when set, clients must send this value in the `X-API-Key` request header on all endpoints except `/health`. The same key is enforced on the Rust backend for mutating operations. Leave unset for local development with no authentication.
* `rate_limit_per_minute` - *(optional, default `0` = disabled)* maximum requests per client IP per minute on all endpoints except `/health`. Applies to both the Python and Rust REST APIs. Uses the first address in `X-Forwarded-For` when present.
* `max_broadcast_tx_bytes` - *(optional, default `1000000`)* maximum decoded transaction size accepted by `POST /tx/hex` and the Rust `POST /tx/raw` broadcast endpoint. Requests above this limit are rejected before parsing.

For production deployments, bind the Python API to a private interface (for example `127.0.0.1:5010`) or place the service behind a reverse proxy. Do not expose the Rust API port (`8081`) or the database/admin ports to the public internet without additional network controls. See [Security](Security.md) for details.

