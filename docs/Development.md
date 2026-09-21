
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

Some tests need a PostgreSQL server and are skipped, not failed, when
`UAAS_TEST_POSTGRES_URL` is unset. Point it at a throwaway database — these
tests write to, and delete from, the tables they use:
```bash
export UAAS_TEST_POSTGRES_URL=postgresql://uaas:uaas-password@127.0.0.1:5432/uaas_test_db
cd rust
# The tests no longer create their own tables: the migrations own the schema.
# Without this they fail on a missing relation rather than skipping.
cargo run -- migrate "$UAAS_TEST_POSTGRES_URL"
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
## The adversarial fixture corpus

`rust/src/uaas/probes.rs` holds a corpus of probes against the collection
matcher, each one a reconstruction of a lettered probe from the adversarial
review. Most of them assert behaviour we intend to change — a matcher that is a
substring search over bytes cannot tell "this output pays the monitored key"
from "these bytes appear somewhere in this output". Those probes are named
`..._today` and carry a `TODO(CS-415)` comment; when the structural matcher
lands, the assertion inverts and the suffix goes.

```bash
cd rust
cargo test probe_
```

Fixtures are written with the assembler in `rust/src/uaas/script_asm.rs` rather
than as hex literals, so a push encoding is stated rather than hand-counted:

```
OP_RETURN 0x76a914c0d164cbb336e3c64338c70506ef543c2fc7b8f988ac
```

Both modules are `#[cfg(test)]` and live in `src/` rather than `rust/tests/`.
An integration test is a separate crate and links the library built *without*
`cfg(test)`, so neither module exists from there.

**No probe may connect to, or replay from, a mainnet node.** Synthetic fixtures
and testnet only, until explicitly cleared.

### Benchmark probes

Throughput probes are gated behind `UAAS_BENCH` so they do not slow down an
ordinary test run, and are only meaningful in a release build:

```bash
cd rust
UAAS_BENCH=1 cargo test --release bench_probe -- --nocapture
```

## Fuzzing

`rust/fuzz/` holds two coverage-guided fuzz targets over the collection
matcher, seeded from the probe corpus above:

| Target | Fuzzed input | Property |
|---|---|---|
| `matcher_pattern` | the `locking_script_pattern` string | compiling a caller-supplied pattern never panics or aborts. This is the `POST /collection/monitor` surface. |
| `matcher_script` | raw locking script bytes | matching never panics, and any captured `identifier` is a whole number of bytes. |
| `script_parse` | raw locking script bytes | tokenising terminates, stays inside the input, never panics, and round-trips the script byte for byte. |

### Setup

`cargo-fuzz` needs a nightly compiler: it passes `-Z sanitizer=address` to
rustc, which stable rejects. See the
[Rust Fuzz Book](https://rust-fuzz.github.io/book/cargo-fuzz/setup.html).

```bash
cargo install cargo-fuzz
rustup toolchain install nightly-2026-09-17
```

`rust/fuzz/rust-toolchain.toml` pins that nightly, and the date is pinned on
purpose — a floating channel is what put the 1.98 pin at the repository root
in the first place. Bump it deliberately.

### Running

**Run it from `rust/fuzz/`, not from `rust/`.** rustup picks a toolchain from
the *working directory*, and `cargo fuzz` passes `--manifest-path` rather than
changing directory, so invoking it from `rust/` uses the root 1.98 pin and
fails with `the option 'Z' is only accepted on the nightly compiler`.

```bash
cd rust/fuzz
mkdir -p corpus/matcher_script
cargo fuzz run matcher_script corpus/matcher_script seeds/matcher_script -- -max_total_time=300
```

The first directory is the working corpus, which libFuzzer grows and which is
gitignored. The second is the committed read-only seed set.

Nothing else is affected: `cargo build`, `cargo test` and CI all run from
`rust/` or the repository root and keep the 1.98 pin. `rust/fuzz/Cargo.toml`
declares its own `[workspace]` so the nightly-only crate is never pulled into
an ordinary build.

### Seeds

`rust/fuzz/seeds/` is generated from the probe fixtures, so the two cannot
drift. `write_fuzz_seed_corpus` verifies the committed seeds on every
`cargo test` run and fails if a fixture changed without them being
regenerated:

```bash
cd rust
UAAS_WRITE_FUZZ_CORPUS=1 cargo test --lib write_fuzz_seed_corpus
```

A fuzz run is not a per-push CI job — it wants minutes to hours, so it belongs
in a scheduled or on-demand workflow.

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
Integration tests require PostgreSQL with the schema applied, and are skipped
unless `UAAS_TEST_POSTGRES_URL` is set. **They delete rows**, so the database
name must contain `test`; the suite refuses anything else rather than trusting
you to have read this:
```
export UAAS_TEST_POSTGRES_URL=postgresql://uaas:uaas-password@127.0.0.1:5433/uaas_test_db
(cd rust && cargo run -- migrate "$UAAS_TEST_POSTGRES_URL")
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

The Rust component reads one TOML file, `data/uaasr.toml`, into `Config`.

| Section | Struct | Fields |
|---|---|---|
| `[service]` | `Service` | `user_agent`, `network`, `rust_address` |
| `[mainnet]`, `[testnet]` | `NetworkSettings` | `ip`, `port`, `timeout_period`, `start_block_hash`, `start_block_height`, `startup_load_from_database`, `block_file`, `save_blocks`, `save_txs` |
| `[database]` | `DatabaseConfig` | `postgres_url`, `postgres_url_docker`, `ms_delay`, `retries` |
| `[orphan]` | `OrphanConfig` | `detect`, `threshold` |
| `[mempool]` | `MempoolConfig` | `eviction_blocks` |
| `[logging]` | `LoggingConfig` | `level` |
| `[dynamic_config]` | `DynamicConfigConfig` | `filename` |
| `[web_interface]` | `WebInterfaceConfig` | `api_key`, `rate_limit_per_minute`, `max_broadcast_tx_bytes` |
| `[[collection]]` | `CollectionConfig` | `name`, `track_descendants`, `address`, `locking_script_pattern` |

Three things that are easy to get wrong:

* **The database URL is under `[database]`, not per network.** One database
  serves both networks and both components read the same URL.
* `[mempool]`, `[web_interface]` and `[[collection]]` are `#[serde(default)]`,
  so a file that omits them still loads. The rest are required, and a missing
  one is a startup failure rather than a default.
* `CollectionConfig` is not only the TOML shape. It is also the
  `POST /collection/monitor` request body and the serialised form of the
  dynamic monitor file, so a field added here lands in all three.


# Configuration is mounted, never baked in

`uaas-service` ships the binary and nothing else. It has no `/app/data` at all,
so a configuration file must be supplied at run time:

```bash
docker run -v "$PWD/data:/app/data:ro" uaas-service
```

`docker-compose.yml` already does this. Started without it, the service exits
non-zero and names the path it wanted:

```
Fatal startup error: no configuration at /app/data/uaasr.toml. The image ships
without one: mount a directory containing uaasr.toml at /app/data, or set
UAASR_CONFIG to the configuration as JSON.
```

`UAASR_CONFIG` takes the whole configuration as JSON and skips the file
entirely, which suits an orchestrator that injects configuration as an
environment variable.

**Why the image carries none.** `Rust_Dockerfile` used to copy
`data/uaasr.toml` into the builder and then into the release stage.
`data/uaasr.toml` holds database credentials for both networks and the peer IP
list, and `multi-build.sh` publishes that image to `nchain/innovation-uaas-service`.
Compose mounts a different file over the top at run time, so the baked copy was
never *used* — but it stayed in the image layer, readable by anyone who pulled
the tag:

```console
$ docker run --rm --entrypoint sh uaas-service -c 'grep postgres_url /app/data/uaasr.toml'
postgres_url = "postgresql://uaas:uaas-password@localhost:5433/uaas_db"
```

A runtime mount hides a file; it does not remove it from the image. See CS-401.

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


