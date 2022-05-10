# Project Status
This section contains project status related notes.

## Done
* Set up database connection
* Connect to database in Rust
* Add `toml` configuration file
* get inv messages working
* Open MySQL database in Python
* Get basic P2P messages working
* Set up basic project
* need to inform child threads their time is up....
* Add offline detection - done now captures when asleep
* write block headers to database
* write mempool to database
    * add `time` (when added) to mempool so that we can determine when tx expires
* remove status table
## In Progress

* address issue of only receiving 4 blocks after a get block message.
    * works on testnet
## TODO

* Request additional blocks
* need to check on mainnet
* May need to support larger p2p messages
    * add len() to script

* add secondary mempool
* search for todos

* need to save state so the same information is not reprocessed
    * need a state table
    * save last block processed

* write block headers to database
* write utxo set to database
* write tx to database

* unable to write blobs to database

* optimise database types
* bulk write tx from block into tx table.

* add logging


# Memory usage
* 05/05/2022 - 242 MB
* 06/05/2022 - 261 MB

-----
* Python database interface

* Connect to `mainnet` and `testnet`
* Prove `addr` message received

* Print out time and peer with event (CSV)
* Timeout if message not received for a period..
* Get peer user agent string etc.
* Connect to multiple peers concurrently
* Manage child threads
* Read config from env vars
* Add database
* Add rust build and release to docker file

* Support big messages
* Agree requirements


# Notes
* This service will not have a 'prune' feature. This increased the complexity of the orignal Python UaaS project for limited gain.