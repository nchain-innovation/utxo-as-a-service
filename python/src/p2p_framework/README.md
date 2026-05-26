# Python P2P framework

Vendored Bitcoin-style serialization and (legacy) P2P networking code used by the Python REST API layer.

P2P sync and block download are handled by the **Rust service** (`rust/`). This package is **not** on the hot path for production sync — the Python API uses only the data-structure and hashing modules.

## Active modules (used by the REST API)

| Module | Purpose | Imported by |
|--------|---------|-------------|
| `object.py` | `CTransaction`, `CBlock`, `CBlockHeader`, and related types | `rest_api`, `tx_analyser`, `collection`, `blockfile`, `block_manager`, tools |
| `hash.py` | `hash256`, `sha256`, `siphash256` | `merkle`, `util`, tools |
| `serial.py` | Binary (de)serialization helpers | Used internally by `object.py` |
| `util.py` | Hex/byte conversion utilities | Used internally by `object.py` and `serial.py` |
| `consensus.py` | Protocol constants | Used internally by `object.py` and `message.py` |
| `streams.py` | Stream type helpers for message parsing | Used internally by `message.py` |

## Legacy modules (not used by the running service)

These implement a Python **mininode**-style P2P client (asyncore-based). Nothing in `web.py`, `rest_api.py`, or the main query path imports them. They remain for reference and may be removed in a future cleanup once serialization is extracted.

| Module | Notes |
|--------|-------|
| `node_connection.py` | Low-level peer socket handling |
| `node_callbacks.py` | Default P2P message callbacks |
| `network_thread.py` | Background asyncore network thread |
| `message.py` | P2P message constructors |
| `merkle.py` | Merkle tree helpers (superseded by `../merkle.py` for API use) |

## Maintenance guidance

- Prefer extending the **Rust** service for new P2P or sync behaviour.
- When changing transaction or block parsing in Python, update `object.py` / `hash.py` and run `./lint.sh`.
- Do not add new production dependencies on the legacy networking modules listed above.
