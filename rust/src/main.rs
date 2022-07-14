#[macro_use]
extern crate lazy_static;
extern crate chrono;
extern crate hex;
extern crate rand;
extern crate regex;

use std::net::IpAddr;
use std::thread;

use actix_web::{App, HttpServer, web};

mod config;
mod event_handler;
mod peer_connection;
mod peer_event;
mod peer_thread;
mod services;
mod thread_manager;
mod thread_tracker;
mod uaas;
mod rest_api;

use crate::config::get_config;
use crate::thread_manager::ThreadManager;
use crate::thread_tracker::ThreadTracker;
use crate::uaas::logic::Logic;
use crate::rest_api::{AppState, broadcast_tx};


#[actix_web::main]
async fn main() {
    let config = match get_config("UAASR_CONFIG", "../data/uaasr.toml") {
        Some(config) => config,
        None => panic!("Unable to read config"),
    };

    // Get web server address from config
    let server_address = config.service.rust_address.clone();
    // Setup web server data
    let web_state = web::Data::new(AppState::default() );
    let webstate = web_state.clone();
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

    // Start the peer threads
    let handle = thread::spawn(move ||
        // Cycle around all the IP addresses
        for ip in ips.into_iter().cycle() {
            manager.create_thread(ip, &mut children, &config);
            manager.process_messages(&mut children, &mut logic, &webstate);
        }
    );

    // Start webserver

    let server = HttpServer::new(move || {
        App::new()
            .app_data(web_state.clone())
            .service(broadcast_tx)
    })
    .workers(1)
    .bind(server_address).unwrap();
    server.run().await.unwrap();

    // wait for peer threads
    handle.join().unwrap();
}
