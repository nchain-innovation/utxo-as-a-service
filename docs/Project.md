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
* write mempool to database
    * add `time` (when added) to mempool so that we can determine when tx expires

* need to save state so the same information is not reprocessed
    * need a state table
    * save last block processed
    * remove status table - we can determine this information from the blocks table

* address issue of only receiving 4 blocks after a get block message.
    * works on testnet
* Write block headers to database
* Write tx to database - txid and block index written
    * bulk write tx from block into tx table.

* write mempool to database
    * add `time` (when added) to mempool so that we can determine when tx expires
    * add `fee` so that we can determine the min fee

* Dont write duplicate blockheaders to blocks.
* Time between block requests should be configurable (logic.rs)

## In Progress

* Check on mainnet
* Write utxo set to database

## TODO

* Request additional blocks
* Mainnet nodes disconnect
    * appears to work if you request the tip (or close to it)
    * even if you set the start_height: 738839 in version
    * we get disconnected if we keep asking for the current tip


* May need to support larger p2p messages
    * add len() to script

* add secondary mempool - what determines secondary mempool ?
    * https://wiki.bitcoinsv.io/index.php/Transaction_Pools


* search for todos

* Read headers using REST API


* unable to write blobs to database

* optimise database types

* add logging


# Memory usage
* 05/05/2022 - 242 MB - mainnet
* 06/05/2022 - 261 MB - mainnet
* 10/06/2022 - 51 MB - testnet
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