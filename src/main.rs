#[macro_use]
extern crate lazy_static;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::mpsc;
use std::thread;

mod config;
mod event_handler;
mod peer;
mod services;

use crate::config::read_config;
use crate::event_handler::EventType;
use crate::peer::connect_to_peer;

#[derive(Debug)]
enum PeerThreadStatus {
    Started,
    Connected,
    Disconnected,
    Finished,
}

#[derive(Debug)]
struct PeerThread {
    pub thread: Option<thread::JoinHandle<()>>,
    pub status: PeerThreadStatus,
}

struct ThreadTracker {
    // Used to track peer connection threads
    children: HashMap<IpAddr, PeerThread>,
}

impl ThreadTracker {
    fn new() -> Self {
        ThreadTracker {
            children: HashMap::new(),
        }
    }

    fn add(&mut self, ip: IpAddr, peer: PeerThread) {
        self.children.insert(ip, peer);
    }

    fn print(&self) {
        for (ip, child) in &self.children {
            println!("ip = {}, result={:?}", ip, child);
        }
    }

    fn set_status(&mut self, ip: &IpAddr, status: PeerThreadStatus) {
        // note this quietly fails if not found
        if let Some(x) = self.children.get_mut(ip) {
            x.status = status;
        }
    }

    fn join_thread(&mut self, ip: &IpAddr) {
        // Joins the thread (wait for it to finish)
        // remove required to move thread out of hashmap
        if let Some(peer) = self.children.remove(ip) {
            if let Some(thread) = peer.thread {
                // wait for it
                let result = thread.join().unwrap();
                println!("result={:?}", result);

                // Create a new entry to replace the existing one
                let new_peer = PeerThread {
                    thread: None,
                    status: PeerThreadStatus::Finished,
                };
                self.children.insert(*ip, new_peer);
            }
        }
    }
}

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
                children.print();
                // wait for thread
                children.join_thread(&received.peer);
                children.print();
            }
            _ => {}
        }
    }
}
