# BNAR Bitcoin Network Analyser - Rust Implementation

 The BNA (Bitcoin Network Analyser) communicates with peer BSV Nodes and captures information that provides an insight as to the status of the BSV network.

This project uses the following Bitcoin SV Rust library for processing peer to peer (P2P) messages:
https://github.com/brentongunning/rust-sv


## To Build the Project
The project is developed in Rust.
The best way to install Rust is to use `rustup`, see https://www.rust-lang.org/tools/install

To build:
```bash
cargo build
```

To run:
```bash
cargo run
```


## Directories
The following directories exist in this project:
```
├── data
├── docs
└── src
```
These directories contain the following:
* `data` - Configuration, data and logs used and created by the service
* `docs` - Project Documentation
* `src` - Project source code in Rust

# Project Notes
This section contains project status related notes.

## Done
* Get basic P2P messages working
* Connect to `mainnet` and `testnet`
* Add `toml` configuration file
* Prove `addr` message received
* Print out time and peer with event (CSV)
* Timeout if message not received for a period..
* Get peer user agent string etc.
* Connect to multiple peers concurrently


## In Progress
* Manage child threads

## TODO
* Agree requirements
* Add rust build and release to docker file
* Add database
* Read config from env vars
