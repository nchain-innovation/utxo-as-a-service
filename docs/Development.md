
# Rust Development
As noted in the main README.md, the best way to install Rust is to use `rustup`.
Use a recent stable Rust toolchain (edition 2021).

Once installed update rust toolset using:
```bash
rustup update
```

To run unit tests:
```bash
cd rust
cargo test
```

To format the code:
```bash
cd rust
cargo fmt
```

For Rust hints:
```bash
cd rust
cargo clippy
```
## Orphan testing
The rust service has `rnd_orphans` a feature flag which introduces random orphans into the download stream.
To test try the following
```
cargo run --features "rnd_orphans"
```


# Python Development
Install [uv](https://docs.astral.sh/uv/) and sync dependencies from the project root:
```
uv sync --all-groups
```
To lint the source code:
```
./lint.sh
```
To run tests:
```
uv run pytest python/tests -v
```
Integration tests require MariaDB and are skipped unless `UAAS_TEST_MYSQL_URL` is set:
```
export UAAS_TEST_MYSQL_URL=mysql://maas:maas-password@127.0.0.1:3306/main_uaas_db
uv run pytest python/tests -v
```
This requires dev dependencies from `pyproject.toml` (`dependency-groups.dev`).

## Python `p2p_framework`

The Python REST API uses a vendored package at `python/src/p2p_framework/` for Bitcoin transaction/block (de)serialization and hashing. **P2P sync runs in Rust**, not in this package. See [`python/src/p2p_framework/README.md`](../python/src/p2p_framework/README.md) for module details.

# Background Links
Details of the messages and the Bitcoin SV peer to peer protocol can be found in the following links:

* https://wiki.bitcoinsv.io/index.php/Peer-To-Peer_Protocol
* https://developer.bitcoin.org/reference/p2p_networking.html


Note as of Bitcoin SV 1.0.11 bloom filters are no longer supported.

# Service Datastructures
The Rust component of the service is constructed of the following components.

![Structs](diagrams/service_structure.png)


# Service Configuration
The Rust component of the service uses the following configuration components.

![Structs](diagrams/config_structure.png)


# Peer Thread Status States
The peer thread works through the following states:

![States](diagrams/threadstates.png)

# Notes
This service processes blocks before reaching the ready state.
However it only processes blocks in the correct order. If blocks arrive out of order they are queued for later processing.



This service writes the blocks to the disk in the correct order and asserts if reading them out of order.

Tx are placed in the mempool prior to the service reaching the ready state.

The ready state means that the service has caught up with the chain tip.

This service only keeps block headers in memory - it writes blocks out to the hard disk.


Note that if the service is off line for a period the mempool may not be correct.
That is to say that it may have missed transaction announcements


