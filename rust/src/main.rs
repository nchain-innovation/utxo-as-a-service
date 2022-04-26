#[macro_use]
extern crate lazy_static;

use std::net::IpAddr;
use std::sync::mpsc;
use std::thread;

mod config;
mod event_handler;
mod peer;
mod services;
mod thread_tracker;
mod uaas;

use crate::config::get_config;
use crate::event_handler::EventType;
use crate::peer::connect_to_peer;
use crate::thread_tracker::{PeerThread, PeerThreadStatus, ThreadTracker};
use crate::uaas::logic::{Logic, ServerStateType};

fn main() {
    // let count = thread::available_parallelism().expect("parallel error");
    // println!("available_parallelism = {}", count);
    // println!("current thread id = {:?}", thread::current().id());

    let config = match get_config("UAASR_CONFIG", "../data/uaasr.toml") {
        Some(config) => config,
        None => panic!("Unable to read config"),
    };

    dbg!(&config);

    // Decode config
    let ips: Vec<IpAddr> = config
        .get_ips()
        .expect("Error decoding config ip addresses");

    let mut logic = Logic::new(&config);

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
                logic.set_state(ServerStateType::Connected);
            }

            EventType::Disconnected => {
                // If we have disconnected then there is the opportunity to start another thread
                children.set_status(&received.peer, PeerThreadStatus::Disconnected);
                // Wait for thread, sets state to Finished
                children.join_thread(&received.peer);
                children.print();
                logic.set_state(ServerStateType::Disconnected);
                if children.all_finished() {
                    break;
                }
            }

            EventType::Tx(tx) => logic.on_tx(tx),
            EventType::Block(block) => logic.on_block(block),
            EventType::Addr(addr) => logic.on_addr(addr),
            EventType::Headers(headers) => logic.on_headers(headers),
        }
    }
}
