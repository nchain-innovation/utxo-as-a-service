# Backfilling the UTXO set from a node's chainstate

The service only knows about outputs it has seen on the wire since it started.
Anything created before that is absent, and there is no way to ask the network
for it: the P2P protocol has no "send me the current UTXO set" message.

A Bitcoin SV node already holds that set, in its `chainstate` LevelDB. This is
how it gets read out and loaded in.

## The shape of it

Four steps, in two repositories, joined by a file:

```
bitcoin_cdbwrapper                          utxo-as-a-service
------------------                          -----------------
scan_chainstate  --->  an export directory  --->  uaas-load-utxo
                            ^
                            |
                       a human reviews it
```

The join is a file and not a library, deliberately:

* **Nothing** from bitcoin-sv, boost or LevelDB enters the UaaS build.
* **The review step becomes structural rather than procedural.** Two programs
  joined by a file cannot skip it by accident. There is no code path from a
  chainstate to the database that does not pass through something a person
  looked at.

## Step 1: get the chainstate off the node safely

Never point either tool at a running node's directory. `leveldb::DB::Open`
takes an exclusive lock and replays the write-ahead log, so it cannot open a
live database and it **modifies** a stopped one.

Take a copy, check it, and work on a clone of the copy:

```bash
# On the node, with bitcoind stopped, or from a crash-consistent snapshot.
rsync -az --partial bitcoin@node:~/bitcoin-testnet/testnet3/chainstate/ ./chainstate/

# Check it before trusting it: replays the MANIFEST, checks every crc32c,
# compares the live table set to what is on disk, and walks the WALs.
python3 bitcoin_cdbwrapper/scripts/verify_chainstate_copy.py ./chainstate
```

Then scan a *clone*, not the copy, and delete the clone afterwards. On APFS
`cp -Rc` is copy-on-write, so a 29 GiB clone takes a fraction of a second and
almost no space.

## Step 2: scan it

```bash
scan_chainstate --db ./clone --out-dir ./export \
    --patterns-file data/scan_patterns.toml
```

The pattern file is a **coarse** filter whose only job is to cut hundreds of
millions of records down to a reviewable set. It is allowed to be
over-inclusive and must not be under-inclusive: a false positive costs a row
a reviewer discards, while a false negative can never be recovered downstream,
because chainstate has no random access once the pass is over.

Start with `--stop-after` on a first run against real data. Two code paths —
the 16-byte large-block framing and the `CDiskTxPos` 4 GiB offset escape — are
proven only against synthetic fixtures, because testnet has no block large
enough to produce either, so a mainnet run is the first time they see real
bytes.

**Scripts dominate the output.** A testnet run over 6,000,000 chainstate
records produced 20,983 candidates, a 5.9 MB CSV and **818 MB** of scripts.
Check free space before a full pass.

## Step 3: review it

`candidates.csv` is the artefact a human signs off: one bounded row per
candidate, openable in a spreadsheet. The format is specified in
`bitcoin_cdbwrapper/docs/candidate_export.md`.

Note the `script_is_raw` column. Where it is `false` the node stored a
compressed template, so those bytes are a 20- or 32-byte hash rather than a
script. The loader refuses them rather than matching a locking-script pattern
against a hash.

When the set is approved, record one value — the manifest's SHA-256:

```bash
shasum -a 256 export/export.toml
```

That one hash commits to the whole export, because the manifest records the
digests of the other two files.

## Step 4: load it

**Stop the service first.** `Utxo` caches the whole spendable set in memory at
startup, so rows inserted underneath a running service are invisible to it —
and `Utxo::settle` does nothing at all for an outpoint it does not hold, so a
spend of a freshly-inserted row would be dropped without a trace. The loader
refuses to start while anything else is attached to the database.

```bash
uaas-load-utxo --export ./export \
               --approve <the sha256 from step 3> \
               --config ../data/uaasr.toml \
               --dry-run
```

Point `--config` at the **service's own** configuration. The collections have
to be the ones the service uses: they decide what loads, and a backfilled row
written under different rules from a live one would be indistinguishable from
it afterwards.

Drop `--dry-run` when the totals look right, then **restart the service**.

### What the loader will not do

* **Overwrite anything.** A row already in `utxo` was put there by the service
  from the chain, and it knows more than a snapshot does. Every statement is
  `ON CONFLICT DO NOTHING`, which is also what makes a second run an exact
  no-op rather than a rewrite.
* **Resurrect a spent output.** Chainstate says an output was unspent when the
  snapshot was taken; `utxo_spent` says the service has seen it spent since.
  The service's view is newer.
* **Trust the coarse filter.** The real matcher — the same `hex_pattern` and
  `collection` code the live path uses — decides, and a candidate it rejects is
  counted and skipped.
* **Load an export it cannot identify**: no manifest (an interrupted scan), an
  unknown format version, a digest that does not match, or a manifest hash that
  is not the one approved.

Every total is reported at `warn`. The release profile sets
`release_max_level_warn`, so anything logged at `info` is compiled out and an
operator watching a released binary would see nothing.

## What is deliberately not loaded

* **`blocks`** — cannot be written honestly. `file_offset` indexes UaaS's own
  block file, which does not contain these blocks, and `UNIQUE (height)` would
  collide.
* **`utxo_spent`** — no history is loaded for outputs already spent, so "when
  was this spent" stays unanswerable below the service's start height. The goal
  is the live set.
* **`collection` and `tx`** — possible, since CS-447 can fetch the
  transactions, but a separate decision. A backfilled collection is partial by
  construction: it holds only pattern matches, and `track_descendants` has no
  ancestor chain to walk, so descendant tracking would be incomplete in a way
  that is not visible in the data.

## Shipping

`docker/Rust_Dockerfile` copies only `/app/bin/uaas`. `uaas-load-utxo` is built
but not shipped in the image — a deliberate choice, since a tool that writes
directly to the database is not something a running container needs.
