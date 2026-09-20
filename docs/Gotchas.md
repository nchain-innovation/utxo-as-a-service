# Gotchas

Things in this service that are easy to get wrong and **fail silently** — no
error, no log line, just a wrong answer. Each one is enforced somewhere; this
page says where, so the enforcement can be found before it is removed by
someone who does not know why it exists.

## Txid byte order is reversed between the API and the database

The database stores a txid as 32 raw bytes in **internal** order. Both REST APIs
speak the **display** order, which is the byte-reversal of it. Every hash
crossing that boundary is converted, in both directions.

This changed with the PostgreSQL migration and is the single most likely thing
to be reintroduced. The MariaDB code wrote `Hash256::encode()`, which reverses
before hex-encoding, so the database used to hold display order. The PostgreSQL
code binds `Hash256.0`, which does not. `encode()` now appears only in log
messages.

**Why it is dangerous:** the wrong bytes compared to a `bytea` column match no
rows and raise nothing. An endpoint answers "not found" for everything, or
returns a txid backwards, and nothing anywhere reports a problem.

**Identifiers are not reversed.** An identifier is the bytes a locking-script
pattern captured, taken from the script as written.

| Where | What |
|---|---|
| `python/src/hashes.py` | the conversion, and rejection of malformed input |
| `python/tests/test_hashes.py` | the Python half of the pin |
| `rust/tests/hash_order.rs` | the Rust half — same two literals |
| `docs/Database.md` | the narrative version |

The two test files assert the same pair of literals, so neither component can
change convention without the other failing. Both also guard against a
palindromic fixture, which would let a missing reversal pass everything else.

## The postgres client cannot be touched from inside a tokio runtime

The synchronous `postgres` client drives a tokio runtime internally. Calling it
from a thread that is already inside one panics with *Cannot start a runtime
from within a runtime*, and then panics again in the client's destructor during
cleanup — which makes it a **non-unwinding abort**, not a failed request.

Three things follow, and the second and third are the ones that surprise people:

1. `db::build_pool` must not be called inside a runtime.
2. **`Pool::get` must not either.** r2d2 validates a connection as it hands it
   out — `PostgresConnectionManager::is_valid` issues a query — so a checkout
   performs synchronous I/O on the calling thread.
3. **`web::block` is not a way round it.** Tokio's blocking-pool threads still
   carry the runtime context and panic exactly as a worker thread would.

So `main` is deliberately a synchronous `fn`. It builds the pool, checks the
schema version, constructs `Logic` and spawns the peer thread *before* any
runtime exists, and enters one only for the web server. It also holds a `Pool`
clone past the runtime, because closing a client blocks the same way.

The one place the web layer touches the pool, `rest_api::check_database`, spawns
a plain thread to do it.

**Why it is dangerous:** `cargo test` cannot see this. Nothing exercises
`main`'s startup path, so the whole suite stayed green while the service aborted
on every start. The guard is the Docker CI job, which starts the service for
real — see `docker/ci/compose.smoke.yml`.

| Where | What |
|---|---|
| `rust/src/db.rs` | the constraint, in full, on `build_pool` |
| `rust/src/main.rs` | why `start()` is synchronous |
| `rust/src/rest_api.rs` | the plain thread in `check_database` |

## The Python integration suite deletes rows

`python/tests/integration/helpers.py` issues unconditional `DELETE`s. Pointed at
a running deployment it destroys indexed chain data, and a production connection
string differs from a test one by a few characters.

`guard_test_database` refuses any database whose name does not contain `test`,
with an override that is deliberately awkward to type. Use a throwaway
container.

The suite also defines no schema of its own — `rust/migrations/` owns it, and
`uaas migrate` applies it. A reachable but unmigrated database is a **failure**,
not a skip, because a skip there is indistinguishable from a pass.

## Operational constraints

* **No mainnet node.** Testnet and synthetic fixtures only, until explicitly
  cleared. The CI smoke test pins every peer address to `192.0.2.1`
  (TEST-NET-1, RFC 5737, unroutable) and fails rather than proceeding if any
  address survives that rewrite.
* **`release_max_level_warn`** is set on the `log` crate, so `info!` and below
  are compiled out of release builds. A message missing from a container log is
  not necessarily a message that was never reached.
