#[macro_use]
extern crate lazy_static;

use std::net::IpAddr;
use std::sync::mpsc;
use std::thread;

mod config;
mod event_handler;
mod peer;
mod services;

use crate::config::read_config;
use crate::peer::connect_to_peer;

fn main() {
    // let count = thread::available_parallelism().expect("parallel error");
    // println!("available_parallelism = {}", count);
    // println!("current thread id = {:?}", thread::current().id());

    // Read config
    let config = match read_config("data/bnar.toml") {
        Ok(config) => config,
        Err(error) => panic!("Error reading config file {:?}", error),
    };
    //dbg!(&config);

    // Decode config
    let ips: Vec<IpAddr> = config
        .get_ips()
        .expect("Error decoding config ip addresses");

    // Set up channels
    let (tx, rx) = mpsc::channel();

    // Connect to peer
    let mut children = vec![];

    // Start the threads
    for ip in ips.into_iter() {
        let local_config = config.clone();
        let local_tx = tx.clone();
        children.push(thread::spawn(move || {
            connect_to_peer(ip, local_config, local_tx)
        }));
    }

    for received in rx {
        println!("{}", received);
    }

    for child in children {
        let result = child.join().unwrap();
        println!("result={:?}", result);
    }
}
