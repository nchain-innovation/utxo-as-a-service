
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


