# UTXO as a Service - Rust Implementation

 The UTXO as a Service (UaaS) monitors BSV Node Peer to Peer (P2P) messages and builds its own UTXO set that can be queried to obtain non-standard transactions.

This uses service implemented in Rust with a Python REST API web interface.
The two components read the same configuration file and share data using database and a shared data directory.

![Service Deployment](docs/diagrams/deployment.png)


This project uses the following Bitcoin SV Rust library for processing peer to peer (P2P) messages:
https://github.com/brentongunning/rust-sv


## To Build the Project
The project is developed in Rust.
The best way to install Rust is to use `rustup`, see https://www.rust-lang.org/tools/install

To build:
```bash
cd rust
cargo build
```
Note that this projec


To run:
```bash
cd rust
cargo run
```

## Database
This service writes the P2P messages to a `MySQL` database.
Database setup details can be found [here](docs/Database.md).

## Docker
Encapsulating the service in Docker removes the need to install the project dependencies on the host machine.
Only Docker is required to build and run the service.
### 1) Build The Docker Image
To build the docker image associated with the service run the following comand in the project directory.
```bash
cd python
./build.sh
```
This builds the docker image `uaas-web`.
### 2) To Run the Image
Once the `uaas-web` image has been build, to run the service use the following script:
```bash
cd python
./run.sh
```
## Web Interface
The service provides a REST API with a Swagger interface at http://localhost:5010/docs

![Rest Api](docs/diagrams/UaaS_REST_API.png)

The service needs to be started with the `-web` command line parameter
The service with webserver application can be started in the Docker container as follows:

## Directories
The following directories exist in this project:
```
├── data
├── docs
│   └── diagrams
├── rust
│   └── src
└── python
    └── src

```
These directories contain the following:
* `docs` - Project documentation
* `docs/diagrams` - PlantUML diagrams and source in support of the documentation
* `rust/data` - Configuration, data and logs used and created by the service
* `rust/src` - Service source code in Rust
* `python/src` - Python REST web interface to UaaS


## Developemnt
Project development details can be found [here](docs/Development.md).

Project status notes can be found [here](docs/Project.md).
