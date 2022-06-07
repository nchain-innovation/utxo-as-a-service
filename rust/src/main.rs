#[macro_use]
extern crate lazy_static;
extern crate chrono;
extern crate hex;
extern crate rand;
extern crate regex;

use std::net::IpAddr;

mod config;
mod event_handler;
mod peer_connection;
mod peer_event;
mod peer_thread;
mod services;
mod thread_manager;
mod thread_tracker;
mod uaas;

use crate::config::get_config;
use crate::thread_manager::ThreadManager;
use crate::thread_tracker::ThreadTracker;
use crate::uaas::logic::Logic;

fn main() {
    let config = match get_config("UAASR_CONFIG", "../data/uaasr.toml") {
        Some(config) => config,
        None => panic!("Unable to read config"),
    };

    // Setup logic
    let mut logic = Logic::new(&config);
    logic.setup();

    // Used to track peer connection threads
    let mut children = ThreadTracker::new();
    let mut manager = ThreadManager::new();

    // Decode config
    let ips: Vec<IpAddr> = config
        .get_ips()
        .expect("Error decoding config ip addresses");

    for ip in ips.into_iter().cycle() {
        manager.create_thread(ip, &mut children, &config);
        manager.process_messages(&mut children, &mut logic);
    }
}
