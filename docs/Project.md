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
* Add option to start the service from block file rather than the database
* Add mempool outpoints to the utxo set
* Check height is set correctly after a startup database load
* ---
* Determine the file offset to locate blocks in block file (data/block.dat)
    * found issue where on_block was setting offset to 0

* Get web interface working
    * status message working
    * Read headers using REST API
    * Add ability to get tx
    * add ability to get block

* Improved database performance
    * Optimised database types

* Add collections
    * add min required by John for demo
    * modified collection table to cope with larger tx
    * multiple collections

* split config to make mainnet/testnet switch safer
* ---
* Incorporate both services in docker
    * Rust service to select mysql_url based on Docker container or localhost
* Document configuration file

* Add blocksize and number of tx to blocks table
* Add tx size to transaction table
* remove utxo on tx
06/06/2022 ----
* On disconnect move to next ip address in the list
* Log connect/disconnect to database connect table
* tag correct height on blocks as they arrive
    * update the database to have the correct block height
        * update blocks
        * update tx
* Check on mainnet
    * got working, required change to rust-sv library to support larger blocks
    * Mainnet nodes disconnect
        * Appears to work if you request the tip (or close to it)
        * Still occurs even if you set the start_height: 738839 in version
        * Got disconnected when we kept asking for the current tip

* Store whole transaction in mempool table
* Broadcast tx
* Check broadcast tx hash against known hashes in REST API
* On getting tx we could check to see if the output is spent or not? - done

* Used a separate thread to write to database now takes 5 mins to process blocks and 3 hours to write to database
* Added field blockindex field to to txs to speed up merkle proofs
* Martyn issue was because he let his machine go to sleep whilst performing block downloads
* John's issue was because the block he was downloading was 40MB and we were not using the modified rust-sv library
* Fixed John issue where the logic of reading blocks from file was throwing an assert

---- 05/05/2023
* Make database delay and retry configurable

----  22/05/2023 
* Use chain-gang instead of rust-sv
* Detect orphan blocks
-- 23/05/2023
* Add chain-gang as a submodule
* Derive the start_block_timestamp
-- 24/05/2023
* Reduce the Rust server docker image down to 114 MB (From 5 GB)
* Make orphaning more robust, wait until 500 blocks before enabling,
  also delete tx and utxo entries associated with this block height.

## In Progress

* Merkle Proofs

* Speed up IBD,
    remove index on UTXO
    Build UTXO in memory during IBD
    Build tx and mempool in memory during IBD


## TODO
* Speed up mysql database, see what can be done in config
* Add configuration item to enble/disable blockfiles

* Search for TODOs

* Prevent sql injection attack on string fields - clean entry...

* Store blocks in multiple files
    * index to file mapping
    * maximum file size limit
        * The HFS+ (Mac OS X Extended) maximum file size limit is 8 exabytes, or 8 billion gigabytes  (8,000,000,000 GB)
        * FAT32 - 4Gib  
        * NTFS - 16Eib  
        * ext2/3 - 16Gib - 2Tib (depends from block size)  
        * ext4 - 16Gib - 16Tib  
        * XFS - 9Eib  
        * ZFS - 16Eib
        EiB == 2^60 bytes exbibyte
        Exabyte = 10^18

    * max files in folder
        HFS Plus file system has a theoretical limit of a 2 billion files
        NTFS Maximum number of files in a single folder:
        4,294,967,295.

* Use blocktime to age tx

* May need to support larger p2p messages
    * add len() to script

* add secondary mempool - what determines secondary mempool ?
    * https://wiki.bitcoinsv.io/index.php/Transaction_Pools


* unable to write blobs to database
* add logging
* Update documentation on Configuration settings



# Memory usage
* 05/05/2022 - 242 MB - mainnet
* 06/05/2022 - 261 MB - mainnet
* 10/05/2022 - 51..98.9MB - testnet now with large utxo set
* 11/05/2022 - 15.4MB on loading from database- testnet
* 12/05/2022 - 195 MB on loading from file on startup (189 queued blocks)- testnet

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


Note 2GB block
00000000000000001062062463df0b4560203c0be8256a59b7bdeecfcb58a226

https://whatsonchain.com/block/00000000000000001062062463df0b4560203c0be8256a59b7bdeecfcb58a226

Largest block (Mainnet)
https://whatsonchain.com/block-height/725511?tncpw_session=b439cbfa9ed22a498d11b9a59e89cd87bf82554ceb114c7ee7bc94bd556aab8d


# Issues


## Issue 3
John issue
connecting to db with another db but same username sees tables created on other db

SELECT TABLE_SCHEMA, TABLE_NAME FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_TYPE = 'BASE TABLE';


Processing block 000000000000019e70ac8f03d28c9ce40a75ab87031a88be03119c2a6b3af847
process_block = 000000000000019e70ac8f03d28c9ce40a75ab87031a88be03119c2a6b3af847 2022-06-01 17:41:25
thread '<unnamed>' panicked at 'assertion failed: (left == right)
  left: Some(0),
 right: None', src/uaas/block_manager.rs:243:17
stack backtrace:
   0: rust_begin_unwind
             at /rustc/7737e0b5c4103216d6fd8cf941b7ab9bdbaace7c/library/std/src/panicking.rs:584:5
   1: core::panicking::panic_fmt
             at /rustc/7737e0b5c4103216d6fd8cf941b7ab9bdbaace7c/library/core/src/panicking.rs:143:14
   2: core::panicking::assert_failed_inner
             at /rustc/7737e0b5c4103216d6fd8cf941b7ab9bdbaace7c/library/core/src/panicking.rs:225:17
   3: core::panicking::assert_failed
             at /rustc/7737e0b5c4103216d6fd8cf941b7ab9bdbaace7c/library/core/src/panicking.rs:182:5
   4: uaas::uaas::block_manager::BlockManager::process_block_queue
             at ./src/uaas/block_manager.rs:243:17
   5: uaas::uaas::block_manager::BlockManager::on_block
             at ./src/uaas/block_manager.rs:375:17
   6: uaas::uaas::logic::Logic::on_block
             at ./src/uaas/logic.rs:103:9
   7: uaas::thread_manager::ThreadManager::process_event
             at ./src/thread_manager.rs:95:44
   8: uaas::thread_manager::ThreadManager::process_messages
             at ./src/thread_manager.rs:116:32
   9: uaas::main::{{closure}}::{{closure}}
             at ./src/main.rs:61:13
note: Some details are omitted, run with RUST_BACKTRACE=full for a verbose backtrace.


thread '<unnamed>' panicked at 'called Result::unwrap() on an Err value: SendError { .. }', src/event_handler.rs:45:22
stack backtrace:
   0: rust_begin_unwind
             at /rustc/7737e0b5c4103216d6fd8cf941b7ab9bdbaace7c/library/std/src/panicking.rs:584:5
   1: core::panicking::panic_fmt
             at /rustc/7737e0b5c4103216d6fd8cf941b7ab9bdbaace7c/library/core/src/panicking.rs:143:14
   2: core::result::unwrap_failed
             at /rustc/7737e0b5c4103216d6fd8cf941b7ab9bdbaace7c/library/core/src/result.rs:1749:5
   3: core::result::Result<T,E>::unwrap
             at /rustc/7737e0b5c4103216d6fd8cf941b7ab9bdbaace7c/library/core/src/result.rs:1065:23
   4: uaas::event_handler::EventHandler::send_msg
             at ./src/event_handler.rs:45:9
   5: uaas::event_handler::EventHandler::on_tx
             at ./src/event_handler.rs:95:9
   6: <uaas::event_handler::EventHandler as sv::util::rx::Observer<sv::peer::peer::PeerMessage>>::next
             at ./src/event_handler.rs:180:32
   7: <sv::util::rx::Subject<T> as sv::util::rx::Observer<T>>::next
             at /Users/j.murphy/.cargo/registry/src/github.com-1ecc6299db9ec823/sv-0.2.2/src/util/rx.rs:60:39
   8: sv::peer::peer::Peer::connect_internal::{{closure}}
             at /Users/j.murphy/.cargo/registry/src/github.com-1ecc6299db9ec823/sv-0.2.2/src/peer/peer.rs:290:29
note: Some details are omitted, run with RUST_BACKTRACE=full for a verbose backtrace.
timed out at 240.399890645 seconds


# Issue

Recovery from being asleep - this is a service, should not be run on a laptop that sleeps

1658341900.770295s, 176.9.148.163, Tx=0874519e379014d61842a46ffc07d29000927a891c4d07786317ddeee45594fe
Have been asleep for 2.798 seconds
thread '<unnamed>' panicked at 'called `Result::unwrap()` on an `Err` value: IoError { server disconnected }', src/uaas/tx_analyser.rs:380:14
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

Same error different line


process_block = 000000000000d27a7dcb1f943ac4401b89fc5888fef678b34f64d3be0766714e 2022-07-21 22:46:52
thread '<unnamed>' panicked at 'called `Result::unwrap()` on an `Err` value: IoError { server disconnected }', src/uaas/tx_analyser.rs:353:14
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


Issue - Resolved

Have now added a panic handler so that if any thread panics the service will stop.

```
set_state(Ready)
1658789292.57072s, 167.172.61.80, Tx=639f9bffadc6f5f39e5ddcad45c7434654de23efec93e5c803f71c78d7af5446
Have been asleep for 67.614 seconds
1658796501.962826s, 167.172.61.80, Disconnected
set_state(Disconnected)
thread '<unnamed>' panicked at 'called `Result::unwrap()` on an `Err` value: IllegalState("Not connected")', src/event_handler.rs:74:30
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
a.gordon@8-lm-00250 rust %
```

Here we have a peer.send after the connection has disconnected.

```
    fn on_inv(&self, inv: &Inv, peer: &Arc<Peer>) {
        // On inv message
        let mut objects: Vec<InvVect> = Vec::new();

        for i in inv.objects.iter() {
            match i.obj_type {
                TX | BLOCK => objects.push(i.clone()),
                // ignore all others
                _ => {}
            }
        }
        // Request the txs and blocks in the inv message
        if !objects.is_empty() {
            let want = Message::GetData(Inv { objects });
            peer.send(&want).unwrap();
        }
    }


```