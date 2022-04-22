#[macro_use]
extern crate lazy_static;

use std::net::IpAddr;
use std::sync::mpsc;
use std::thread;

use mysql::prelude::*;
use mysql::*;

mod config;
mod event_handler;
mod peer;
mod services;
mod thread_tracker;

use crate::config::get_config;
use crate::event_handler::EventType;
use crate::peer::connect_to_peer;
use crate::thread_tracker::{ThreadTracker, PeerThreadStatus, PeerThread};

fn create_tables(conn: &mut PooledConn) {
    // Create tables, if required

    // Check for the tables
    let tables: Vec<String> = conn
        .query("SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_TYPE = 'BASE TABLE';")
        .unwrap();

    if tables.iter().find(|x| x.as_str() == "txs") == None {
        conn.query_drop(
            r"CREATE TABLE txs (
            time DOUBLE,
            ip text,
            tx text
        )",
        )
        .unwrap();
    }

    if tables.iter().find(|x| x.as_str() == "blocks") == None {
        conn.query_drop(
            r"CREATE TABLE blocks (
            time DOUBLE,
            ip text,
            block text
        )",
        )
        .unwrap();
    }

    if tables.iter().find(|x| x.as_str() == "addr") == None {
        conn.query_drop(
            r"CREATE TABLE addr (
            time DOUBLE,
            ip text,
            address text
        )",
        )
        .unwrap();
    }
}

fn main() {
    // let count = thread::available_parallelism().expect("parallel error");
    // println!("available_parallelism = {}", count);
    // println!("current thread id = {:?}", thread::current().id());

    let config = match get_config("BNAR_CONFIG", "data/bnar.toml") {
        Some(config) => config,
        None => panic!("Unable to read config"),
    };

    // Connect to database
    let pool = Pool::new(&config.mysql_url).unwrap();
    let mut conn = pool.get_conn().unwrap();

    // Create tables, if required
    create_tables(&mut conn);

    // Decode config
    let ips: Vec<IpAddr> = config
        .get_ips()
        .expect("Error decoding config ip addresses");

    // Set up channels
    let (tx, rx) = mpsc::channel();

    // Used to track peer connection threads
    let mut children = ThreadTracker::new();

    // Start the threads
    for ip in ips.into_iter() {
        let local_config = config.clone();
        let local_tx = tx.clone();
        let peer = PeerThread {
            thread: Some(thread::spawn(move || {
                connect_to_peer(ip, local_config, local_tx)
            })),
            status: PeerThreadStatus::Started,
        };
        children.add(ip, peer);
    }
    // Process messages
    for received in rx {
        println!("{}", received);
        match received.event {
            EventType::Connected(_) => {
                children.set_status(&received.peer, PeerThreadStatus::Connected);
                children.print();
            }

            EventType::Disconnected => {
                // If we have disconnected then there is the opportunity to start another thread
                children.set_status(&received.peer, PeerThreadStatus::Disconnected);
                // Wait for thread, sets state to Finished
                children.join_thread(&received.peer);
                children.print();
                if children.all_finished() {
                    break;
                }
            }

            EventType::Tx(ref hash) => {
                conn.exec_drop("INSERT INTO txs (time, ip, tx) VALUES (:time, :ip, :tx)",
                    params! { "time" => received.get_time(), "ip" => received.get_ip(), "tx" => hash} ).unwrap();
            }

            EventType::Block(ref hash) => {
                conn.exec_drop("INSERT INTO blocks (time, ip, block) VALUES (:time, :ip, :block)",
                    params! { "time" => received.get_time(), "ip" => received.get_ip(), "block" => hash} ).unwrap();
            }

            EventType::Addr(ref detail) => {
                conn.exec_drop("INSERT INTO addr (time, ip, address) VALUES (:time, :ip, :address)",
                    params! { "time" => received.get_time(), "ip" => received.get_ip(), "address" => detail} ).unwrap();
            }
        }
    }
}
