# Project Status
This section contains project status related notes.

## Done
* Set up database connection
* Connect to database in Rust
* Add `toml` configuration file
* get inv messages working
* Open MySQL database in Python

## In Progress
* Get basic P2P messages working
* Set up basic project
* need to inform child threads their time is up....

* address issue of only receiving 4 blocks after a get block message.


## TODO
* Add offline detection?
* Request additional blocks (based on ?)
* need to check on mainnet
* May need to support larger p2p messages
    * add len() to script


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