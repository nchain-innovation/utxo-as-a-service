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
* Request additional blocks

* write mempool to database
    * add `time` (when added) to mempool so that we can determine when tx expires

* Save state so the same information is not reprocessed
    * need a state table
    * save last block processed
    * remove status table - we can determine this information from the blocks table!

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

* Bulk write to utxo set in tx
* Bulk delete from utxo set in tx
* Print created databases on startup

* Fix fee calculation
* Write utxo set to database
* Load txs from database on startup
## In Progress
* Get web interface working
* Check on mainnet


## TODO

* Add option to start the service from block file rather than the database
* add mempool to the utxo set


* Add collections

* Add ability to get tx
* add ability to get block

* Determine the file offset to locate blocks in block file (data/block.dat)

* use blocktime to age tx

* Mainnet nodes disconnect
    * Appears to work if you request the tip (or close to it)
    * Still occurs even if you set the start_height: 738839 in version
    * Got disconnected when we kept asking for the current tip

* May need to support larger p2p messages
    * add len() to script

*
* add secondary mempool - what determines secondary mempool ?
    * https://wiki.bitcoinsv.io/index.php/Transaction_Pools


* Search for TODOs

* Read headers using REST API
* unable to write blobs to database
* optimise database types

* add logging
* Update documentation on Configuration settings

# Memory usage
* 05/05/2022 - 242 MB - mainnet
* 06/05/2022 - 261 MB - mainnet
* 10/06/2022 - 51..98.9MB - testnet now with large utxo set
* 11/06/2022 - 15.4MB on loading from database- testnet

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

Tx with 4000 outputs
https://test.whatsonchain.com/tx/4cfb227905ae215eda4515f0d3c7ced86cecbb7ea964f29fed454094fb11c5be
Note the whole block is filled with them


Tx with non-zero locktime
https://test.whatsonchain.com/tx/52422e31e46673709226c48a4482e180a5f6c02b832e6285b9f908697d2792d6


Block with lots of OP_RETURN and multisig
https://test.whatsonchain.com/block/00000000000217ccba9ce86db2c867cc81c7aceb89ba241f83083890d6a0f6a0
