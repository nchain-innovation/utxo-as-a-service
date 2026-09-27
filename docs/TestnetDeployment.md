# Standing up UaaS on testnet

A runbook for a fresh testnet deployment, and for choosing what it monitors.

The second half — [choosing collections](#choosing-what-to-monitor) — matters
more than it looks. The service only records what a collection selects, and
only from its start height forward. Both of those are decided before the first
block is indexed and are expensive to change afterwards.

> **Where the numbers come from.** The figures in this document were measured
> on 2026-09-27 against a verified copy of the testnet chainstate taken on
> 2026-09-22, covering heights up to 1,759,410. They will drift. The shapes and
> the reasoning hold; re-measure the counts if a decision turns on them.

## Before anything else

**Rotate the node's RPC credentials.** This repository is public and the
testnet node's password has been exposed. Deploying against it first and
rotating later means deploying against known-compromised credentials. See
CS-430.

## 1. The decision that matters: `start_block_height`

Everything else here is mechanical. This is the one that shapes what the
deployment can demonstrate.

The service indexes from its start height forward and has no way to ask the
network for what came before — the P2P protocol has no "send me the current
UTXO set" message. Anything older is absent until it is backfilled from a
node's chainstate (see [Backfill.md](Backfill.md)).

So the start height splits the monitored set in two: what the service indexes
live, and what the backfill has to supply.

Measured against 147,791 inscription UTXOs spanning heights 1,542,621 to
1,759,410:

| `start_block_height` | indexed live | left for the backfill | blocks to sync |
| --- | --- | --- | --- |
| 1,643,523 *(the tracked default)* | 23,433 (15.9%) | 124,358 | ~115,900 |
| 1,700,000 | 9,505 (6.4%) | 138,286 | ~59,400 |
| **1,750,000** | **2,201 (1.5%)** | **145,590** | **~9,400** |
| 1,755,000 | 540 (0.4%) | 147,251 | ~4,400 |
| 1,758,000 | 138 (0.1%) | 147,653 | ~1,400 |

**Around 1,750,000 is the useful choice for a first deployment.** It leaves a
couple of thousand live-indexed outputs — enough to compare a backfilled row
against a live one, which is how the two paths are shown not to have drifted —
while leaving almost the whole set for the backfill to supply. And it syncs
about 9,400 blocks rather than 116,000, which matters on testnet: the tracked
config notes a 331 MB block taking over five minutes, which is why
`timeout_period` there is 1000 seconds rather than the mainnet 2048.

Starting later than about 1,758,000 leaves too little live data to compare
against. Starting at the tracked default means a long sync for no benefit.

**`start_block_hash` and `start_block_height` must agree.** Changing the height
alone leaves the service looking for a hash that is not at that height. Look up
the real hash and set both.

## 2. Two traps in the configuration

**Docker reads a different file.** `docker-compose.yml` mounts
`data/uaasr.docker.toml` over `data/uaasr.toml` inside the backend container.
Editing `data/uaasr.toml` and then running compose changes nothing at all.
Edit the `.docker.toml` copy, or both, and keep the `[[collection]]` blocks
identical between them — a difference there means the containerised service and
a host-run tool disagree about what is monitored.

**Credentials come from the environment, never the file.** Both tracked configs
carry a `CHANGE-ME` placeholder and fail on startup with a message saying so,
rather than dialling anything. Supply:

* `POSTGRES_PASSWORD` — for the postgres container
* `UAAS_POSTGRES_URL` — read by `uaas migrate`, the Rust service and the Python
  web API, in preference to the file

Do not put a working URL in a tracked file. See [Security.md](Security.md).

## 3. Build and start

```bash
./build.sh                      # builds the uaas-service and uaas-web images
```

```bash
POSTGRES_PASSWORD='<strong password>' \
UAAS_POSTGRES_URL='postgresql://uaas:<strong password>@postgres:5432/uaas_db' \
docker compose up -d
```

Compose orders the startup itself: postgres, then `uaas_migrate` as a one-shot
that must exit successfully, then the backend, then the web API. The backend
refuses to start against a schema version it does not recognise, so a failed
migration stops the stack rather than half-starting it.

Nothing creates tables at startup. The schema lives in `rust/migrations/`,
embedded in the binary at compile time, and is applied only by `uaas migrate`.

## 4. Check it is actually working

| what | where |
| --- | --- |
| backend health | `http://<host>:8081/health` |
| web API health | `http://<host>:5010/health` |
| database browser | `http://<host>:8080` (adminer) |

Sync progress is visible as rows arriving in `blocks`. `save_blocks` and
`save_txs` are both `false` in the tracked config; leave them that way unless
something needs the raw data, because they are most of the disk cost.

## 5. Security, because this is a reachable host

The REST API has **no authentication by default** and binds `0.0.0.0:5010`. It
can broadcast transactions and modify collection monitors. Either set `api_key`
under `[web_interface]` or keep the port off the public internet. The mainnet
peer list is deliberately unroutable (`192.0.2.1`, TEST-NET-1) so switching
`network` cannot silently dial a real node.

Full detail in [Security.md](Security.md).

## Choosing what to monitor

The service records only what a `[[collection]]` pattern selects. A collection
added later does **not** re-evaluate rows already in `utxo`, and cannot recover
anything below the start height. So configure the collections you want before
the first sync, or do the sync twice.

### What is actually on testnet

147,791 UTXOs carry the 1Sat inscription envelope
`0063036f7264` — `OP_FALSE OP_IF` then a 3-byte push of `ord`. They occur in
**three shapes**, and the difference decides whether a pattern works:

| shape | count | share |
| --- | --- | --- |
| envelope at the very start of the script | 31,482 | 21% |
| `P2PKH` then envelope | 35,145 | 24% |
| `P2PKH` then `OP_CODESEPARATOR` (`ab`) then envelope | 80,049 | 54% |

By declared content type:

| content type | count |
| --- | --- |
| `application/bsv-20` | 93,862 |
| `text/html` | 23,913 |
| `application/json` | 16,255 |
| `image/png` | 8,026 |
| `text/plain` — in five different spellings | ~2,250 |
| `image/jpeg`, `image/webp`, `application/bsv-21`, others | ~800 |

### The tracked `1sat` collection is narrower than its name

```
0063036f726451126170706c69636174696f6e2f6273762d323000[0-9a-f]*
```

This requires the literal `OP_1`, an 18-byte push of `application/bsv-20`, then
`OP_0`, immediately after the envelope. It selects the 93,862 BSV-20
inscriptions and **misses about 53,900 — 36% of the set**: every HTML, JSON,
PNG, JPEG and BSV-21 inscription on the chain.

If that narrowing is intended, rename it `bsv20`, so nobody reads "1sat" and
assumes coverage. If it is not, widen it.

### Suggested collections

Both verified against real testnet scripts:

```toml
[[collection]]
name = "ordinal"
track_descendants = false
# Every inscription, whatever its content type, wherever the envelope sits.
locking_script_pattern = "0063036f7264[0-9a-f]*"

[[collection]]
name = "ordinal_owned"
track_descendants = false
# Those locked to a key, capturing the owner's hash160 as utxo.identifier.
# The [0-9a-f]* absorbs the optional OP_CODESEPARATOR between the two parts.
locking_script_pattern = "76a914(?<identifier>[0-9a-f]{40})88ac[0-9a-f]*0063036f7264[0-9a-f]*"
```

Measured behaviour against one real script of each shape:

| pattern | shape 1 | shape 2 | shape 3 | plain P2PKH |
| --- | --- | --- | --- | --- |
| tracked `1sat` | yes | no | no | no |
| `ordinal` | yes | yes | yes | no |
| the same, `^` anchored | yes | no | no | no |
| `ordinal_owned` | no | yes | yes | no |

`ordinal_owned` captured `a463e826e8365e5a000834aae4d9d1ca08fa1242` from both
prefixed shapes — the owner's hash160, which lands in `utxo.identifier`.

### Three rules the pattern language imposes

These are properties of `rust/src/uaas/hex_pattern.rs`, confirmed by compiling
each form rather than by reading the documentation.

**Do not anchor with `^`.** It costs 78% of the set: only 21% of inscriptions
sit at the start of their script.

**Do not match on content-type text.** There are five spellings of `text/plain`
alone, one of them wrapped in literal quote characters. Match the envelope and
let the content type be data.

**There is no alternation.** `|`, `(...)?`, `.` and arbitrary character classes
are all rejected — deliberately, because the notation is translated to a
byte-level regex and anything it cannot translate faithfully is a configuration
error rather than something to approximate. One pattern describes one shape, so
variants need separate `[[collection]]` entries.

The only supported notation is: literal hex, `[0-9a-f]{N}` for N/2 bytes,
`[0-9a-f]*` and `[0-9a-f]+`, `^` and `$`, and one `(?<identifier>...)` capture.

### The coarse filter is a different thing

`bitcoin_cdbwrapper`'s `data/scan_patterns.toml` carries the bare envelope
`0063036f7264`. That is correct and should stay **wider** than anything
configured here: its only job is to cut hundreds of millions of chainstate
records down to a reviewable set, and this matcher is the authority that
decides what actually loads. A false positive there costs a row a reviewer
discards; a false negative cannot be recovered without another full scan.

## When you come to backfill

Follow [Backfill.md](Backfill.md). Two things specific to a live deployment:

**Stop the backend before loading.** `Utxo` caches the whole spendable set in
memory at startup, so rows inserted underneath a running service are invisible
to it — and `Utxo::settle` does nothing at all for an outpoint it does not
hold, so a spend of a freshly inserted row is dropped without a trace.
`uaas-load-utxo` refuses to run while anything is attached to the database, and
says so.

**Restart it afterwards**, or the backfilled rows stay invisible to the service
that is meant to be watching them.

**Note on `track_descendants`.** A backfilled collection has no ancestor chain
to walk, so descendant tracking is incomplete for backfilled rows in a way that
is not visible in the data. That is why `collection` and `tx` are out of scope
for the loader.
