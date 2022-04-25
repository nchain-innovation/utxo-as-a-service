# BNAR Bitcoin Network Analyser - Rust Implementation

 The BNA (Bitcoin Network Analyser) communicates with peer BSV Nodes and captures information that provides an insight as to the status of the BSV network.

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

To run:
```bash
cargo run
```

## Run in Docker
Alternatively the project can be executed in a Docker container.  Docker removes the need to install the project dependencies on the host machine.
Only Docker is required to build and run the service.

### 1) Build The Docker Image
To build the docker image associated with the service run the following comand in the project directory.
```bash
./build.sh
```
This builds the docker image `bnar`.
### 2) To Run the Image
Once the `bnar` image has been build, to run the service use the following script:
```bash
./run.sh
```

## Database
This service writes the P2P messages to a `MySQL` database.
Database setup details can be found [here](docs/Database.md).



## Directories
The following directories exist in this project:
```
├── docs
│   └── diagrams
└── rust
    ├── data
    └── src

```
These directories contain the following:
* `docs` - Project documentation
* `docs/diagrams` - PlantUML diagrams and source in support of the documentation
* `rust/data` - Configuration, data and logs used and created by the service
* `rust/src` - Project source code in Rust


## Developemnt
Project development details can be found [here](docs/Development.md).

Project status notes can be found [here](docs/Project.md).
