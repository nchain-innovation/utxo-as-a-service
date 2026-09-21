# UTXO as a Service - Rust Implementation

 The UTXO as a Service (UaaS) monitors BSV Node Peer to Peer (P2P) messages and builds its own UTXO set that can be queried to obtain non-standard transactions.

This service is implemented in Rust with a Python REST API web interface.
The two components read the same configuration file and a shared data directory, and both store their data in **PostgreSQL**.
The diagram also shows the Docker containers that make up the service.
```mermaid
flowchart TB
    user([user])
    peer((BSV PeerNode))

    subgraph compose["docker compose"]
        direction TB
        migrate["uaas_migrate<br/><i>one-shot: applies rust/migrations/</i>"]
        web["uaas_web<br/><i>REST API, Python</i>"]
        rust["uaas_backend<br/><i>indexer, Rust</i>"]
        db[("uaas_postgres<br/>PostgreSQL")]
    end

    user -->|HTTP| web
    peer <-->|P2P messages| rust
    web -->|broadcast_tx| rust
    migrate ==>|schema first| db
    rust -->|writes| db
    db -->|queries| web
```

`uaas_migrate` runs to completion before the indexer starts, and the indexer
refuses to run against a schema version it does not expect — nothing creates
tables at startup.

Both components read the same `uaasr.toml` from `/app/data`, which is
**mounted**: the published image deliberately carries no configuration. The
same directory holds `blocks.dat`, which the indexer writes and the web service
reads.

The database holds eleven tables: `blocks`, `orphans`, `tx`, `mempool`, `utxo`,
`utxo_spent`, `utxo_unmined_spend`, `utxo_monitor`, `collection`, `addr` and
`connect`. See [docs/Database.md](docs/Database.md).

The service stores blocks and can return transactions from those blocks.

If you need transactions that are not in blocks but are in the mempool you will need to set up a `Collection` which
will capture all transactions that match a particular pattern.
For more details on setting up a `Collection` see the configuration documentation [here](docs/Configuration.md).

This project uses the following Chain-Gang Rust library for processing peer to peer (P2P) messages:
https://github.com/nchain-innovation/chain-gang



## Run UaaS in Docker Compose
Docker Compose starts the components that make up the UaaS system in one command.
First build the images using the `build.sh` command.
```bash
./build.sh
```
Then start the system:
```bash
docker-compose up -d
```
Compose publishes PostgreSQL on host port **5433**, off the default to avoid conflicting with other local databases. A one-shot `uaas_migrate` service applies the schema before the indexer starts.

To stop the system:
```bash
docker-compose down
```


## To Build the Service
The service is developed in Rust.
The best way to install Rust is to use `rustup`, see https://www.rust-lang.org/tools/install

To build:
```bash
cd rust
cargo build
```

## To Run the Service
Note that this project requires PostgreSQL running, with the schema applied.
See the `Database` section below for details.

To run:
```bash
cd rust
cargo run
```

If the following message is seen in the output, the service is unable to connect to the database. Check that PostgreSQL is running and that `database.postgres_url` in `data/uaasr.toml` points at the correct host and port.
```
Fatal startup error: Problem connecting to database. Check the database is running and database.postgres_url is correct: could not connect to PostgreSQL: error connecting to server: Connection refused (os error 111)
```
## To Run the REST Web interface

The REST Web interface has been developed in Python.

To run this:
```bash
cd python/src
./web.py
```
Note again that this is dependent on the PostgreSQL database.

This will provide a REST API with a Swagger interface at http://localhost:5010/docs



## Database
The Rust service records data to a PostgreSQL database, which must be present and carry the expected schema version for the service to start.
Database setup details can be found [here](docs/Database.md).

### Schema

The schema is defined in `rust/migrations/`, one object per file, and applied by the service binary:

```bash
uaas migrate postgresql://uaas:uaas-password@localhost:5433/uaas_db
```

It is safe to run repeatedly — migrations already applied are skipped — and it takes `UAAS_POSTGRES_URL` if no URL is given. The migrations are embedded in the binary at compile time, so the published image needs no `psql` and no source tree.

Nothing creates tables at startup any more. The service asserts the schema version it expects and refuses to run against anything else, rather than creating what it finds missing.

Both components read the same database and the same connection URL. See [docs/Database.md](docs/Database.md).

## Docker
Encapsulating the service in Docker removes the need to install the project dependencies on the host machine.
Only Docker is required to build and run the service and web interface.
Note that the PostgreSQL docker image is still required.
### 1) Build The Docker Image
To build the docker image associated with the service, run the following command in the project directory.
```bash
./build.sh
```
This builds two Docker images:
* `uaas-service` for the Rust service
* `uaas-web` for the Python REST API
If there is an out of disk space error whilst building the images, use the following command to free up disk space:
```bash
docker system prune
```
### 2) To Run the Image
As there are two Docker images there are also two startup scripts:
* `run_service.sh` - to start the Rust service
* `run_web.sh` - to start the Python REST API

## Configuration
The configuration of the service is set in `data/uaasr.toml` file.
This is read when the service starts up.

For more details about the configuration file see [here](docs/Configuration.md).

## Security

The REST APIs have no authentication by default and can broadcast transactions or modify collection monitors. For local development, bind to `127.0.0.1` and keep ports off the public internet. For shared or production use, set `api_key` in `[web_interface]` and read [docs/Security.md](docs/Security.md).


## Directories
The following directories exist in this project:
```
├── data
├── docs
│   └── diagrams
├── python
│   └── src
└── rust
    ├── fuzz
    └── src
```
These directories contain the following:
* `data` - Configuration, data and logs used and created by the service
* `docs` - Project documentation
* `docs/diagrams` - PlantUML diagrams and source in support of the documentation
* `python/src` - Python REST web interface to UaaS
* `rust/src` - Rust service source code (P2P sync, UTXO maintenance, internal API)
* `rust/fuzz` - Fuzz targets over the collection matcher. A separate crate on its own toolchain; not part of an ordinary build

## Development
The following diagram shows how the Rust UaaS processes individual `transactions` and `blocks` from peer nodes.
![Usecase](docs/diagrams/usecase.png)

The point to note that as `transactions` (or `tx`) are received they are:
1) the `tx` added to the `mempool` database table
2) the `tx` input `outpoints` are removed from the `UTXO` table
3) the `tx` output `outpoints` are added to the `UTXO` table

When `blocks` are received:
1) the `tx` are removed from the `mempool` and added to the `txs` table
2) the `tx` input `outpoints` (if present) are removed  from the `UTXO` table
3) the `tx` output `outpoints` are added/updated to the `UTXO` table
4) the Block's `blockheader` is added to the `Blocks` table

Another point to note is that this means that blocks and transaction can be processed prior to the block tip being obtained.

The only constraint is that the blocks must be processed in order. This is achieved by ensuring that the `prev_hash` field of the block matches the `hash` of the last block processed, all other blocks are placed on a queue for later processing.

Project development details can be found [here](docs/Development.md).

Systems requirements and verification traceability are documented [here](docs/SystemsRequirements.md).

Project status notes can be found [here](docs/Project.md).

## Fuzzing

`rust/fuzz` holds three coverage-guided fuzz targets over the script matcher and tokeniser:

* `matcher_pattern` — fuzzes the `locking_script_pattern` string. This is the surface `POST /collection/monitor` exposes, so in production every byte of it is chosen by the caller. The property is that compiling a pattern never panics and never aborts, whatever it is handed. Most inputs are rejected, and rejecting is the correct answer.
* `matcher_script` — fixes the pattern and fuzzes the locking script bytes, which is the direction that matters for the indexer: a script arrives from the P2P network inside a transaction. As well as never panicking, a match must yield an identifier that is a whole number of bytes — a capture of any other length would mean a match could straddle a byte boundary.
* `script_parse` — fuzzes the tokeniser over the same script bytes. Tokenising must terminate (every token consumes at least one byte), must never read outside the input however large a push declares itself to be, and must round-trip the script byte for byte from the token stream alone.

### One-off setup

`cargo-fuzz` needs a nightly compiler, because it passes `-Z sanitizer=address` to rustc and stable rejects unstable flags. See the [Rust Fuzz Book](https://rust-fuzz.github.io/book/cargo-fuzz/setup.html).

```bash
cargo install cargo-fuzz
rustup toolchain install nightly-2026-09-17
```

`rust/fuzz/rust-toolchain.toml` pins that nightly, and rustup applies it by directory, so **nothing else in the project moves off the stable 1.98 pin** — not the service, not `cargo test`, not CI.

### Running

Run from `rust/fuzz`, **not** from `rust`:

```bash
cd rust/fuzz
cargo fuzz list
mkdir -p corpus/matcher_script
cargo fuzz run matcher_script corpus/matcher_script seeds/matcher_script -- -max_total_time=300
```

The first directory is the working corpus, which libFuzzer grows as it finds new coverage and which is not committed. The second is the committed seed set, read only — those inputs come from the adversarial probe fixtures in `rust/src/uaas/probes.rs`, so the fuzzer starts from the shapes the review found interesting rather than from random bytes.

Swap `matcher_script` for `matcher_pattern` to run the other target. `-max_total_time` is in seconds; without it the run continues until interrupted.

A crash is written to `rust/fuzz/artifacts/<target>/` and replayed with:

```bash
cargo fuzz run matcher_script artifacts/matcher_script/<crash-file>
```

**Why `rust/fuzz` and not `rust`:** `cargo fuzz` passes `--manifest-path` rather than changing directory, while rustup picks a toolchain from the working directory. Started from `rust`, it is handed the stable pin and fails with `error: the option 'Z' is only accepted on the nightly compiler`.

There is no fuzzing job in CI. A useful run takes minutes to hours, which does not fit a per-push workflow — run it on demand, or after changing the matcher or the pattern grammar.

Seed regeneration and the rest of the detail are in [docs/Development.md](docs/Development.md).

## Building and Publishing Docker Images

This project includes two Docker images: ```uaas-web``` and ```uaas-service```. These images are essential components of UTXO as a Service (UAAS). By publishing them to Docker Hub, they become accessible for use by other projects and applications within the ecosystem.

Both images can be built and published to Docker Hub using a single script.

To build and publish the images, run the following command:
```
./multi-build.sh
```

**Requirements**

- **Docker Buildx:** The script requires Docker's Buildx extension to be set as the active builder. Ensure Buildx is properly installed and selected as the current Docker engine. For help, see [Docker Buildx](https://docs.docker.com/build/builders/)  

- **Publishing Permissions:** Only members of the ```innovation``` team within the ```nChain``` Docker Hub organisation are authorised to publish images with the appropriate tags. Ensure you are logged in with the necessary permissions before running the script, else this will fail.

To login to Docker Hub at the command line use:
```
docker login -u <name>
```
When prompted enter the password.



