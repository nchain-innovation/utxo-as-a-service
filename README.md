# BNAR Bitcoin Network Analyser Rust Implementation




This project uses the following Bitcoin SV Rust library for processing peer to peer (P2P) messages:
https://github.com/brentongunning/rust-sv

# Background Links
Details of the messages and the peer to peer protocol can be found in the following links:

* https://wiki.bitcoinsv.io/index.php/Peer-To-Peer_Protocol
* https://developer.bitcoin.org/reference/p2p_networking.html


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
This section contains project related notes.

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

## TODO
* Agree requirements
* Add rust build and release to docker file
* Add database
* Read config from env vars
