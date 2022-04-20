#[macro_use]
extern crate lazy_static;

use std::net::IpAddr;
use std::thread;

mod config;
mod services;
mod peer;

use crate::config::read_config;
use crate::peer::connect_to_peer;


fn main() {
    let count = thread::available_parallelism().expect("parallel error");
    println!("available_parallelism = {}", count);

    println!("current thread id = {:?}", thread::current().id());

    // Read config

    let config = match read_config("data/bnar.toml") {
        Ok(config) => config,
        Err(error) => panic!("Error reading config file {:?}", error),
    };
    dbg!(&config);
    // Decode config
    let ips: Vec<IpAddr> = config
        .get_ips()
        .expect("Error decoding config ip addresses");

    // Connect to peer
    let mut children = vec![];

    // Start the threads
    for ip in ips.into_iter() {
        let local_config = config.clone();
        children.push(thread::spawn(move || connect_to_peer(ip, local_config)));
    }

    for child in &children {
        // let result = child.join().unwrap();
        let id = child.thread().id();
        //let is_running = child.is_finished();

        println!("id={:?},", id);
        dbg!(child.thread());
    }



    for child in children {
        let result = child.join().unwrap();
        println!("result={:?}", result);
    }

}
