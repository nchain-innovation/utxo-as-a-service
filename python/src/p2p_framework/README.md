# Python P2P framework

Vendored Bitcoin-style serialization used by the Python REST API layer.

P2P sync and block download are handled by the **Rust service** (`rust/`). This package provides transaction and block (de)serialization only.

## Modules

| Module | Purpose | Imported by |
|--------|---------|-------------|
| `object.py` | `CTransaction`, `CBlock`, `CBlockHeader`, and related types | `rest_api`, `tx_analyser`, `collection`, `blockfile`, `block_manager`, tools |
| `hash.py` | `hash256`, `sha256`, `siphash256` | `merkle`, `util`, tools |
| `serial.py` | Binary (de)serialization helpers | Used internally by `object.py` |
| `util.py` | Hex/byte conversion utilities | Used internally by `object.py` and `serial.py` |
| `consensus.py` | Protocol constants | Used internally by `object.py` |

## Maintenance guidance

- Prefer extending the **Rust** service for new P2P or sync behaviour.
- When changing transaction or block parsing in Python, update `object.py` / `hash.py` and run `./lint.sh`.
