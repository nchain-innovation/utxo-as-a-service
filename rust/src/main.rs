#[macro_use]
extern crate lazy_static;

use actix_web::{web, App, HttpServer};
use signal_hook::{consts::SIGINT, iterator::Signals};
use std::{
    net::{IpAddr, Ipv4Addr},
    panic, process,
    sync::mpsc,
    thread, time,
};

mod config;
mod dynamic_config;
mod event_handler;
mod peer_connection;
mod peer_event;
mod peer_thread;
mod rest_api;
mod services;
mod thread_manager;
mod thread_tracker;
mod uaas;

use crate::{
    config::get_config,
    peer_event::{PeerEventMessage, PeerEventType},
    rest_api::{add_monitor, broadcast_tx, delete_monitor, version, AppState},
    thread_manager::ThreadManager,
    thread_tracker::ThreadTracker,
    uaas::logic::Logic,
};

#[actix_web::main]
async fn main() {
    // Hook in our own panic handler
    let orig_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        // invoke the default handler and exit the process
        orig_hook(panic_info);
        process::exit(1);
    }));

    // Read the config
    let config = match get_config("UAASR_CONFIG", "../data/uaasr.toml") {
        Some(config) => config,
        None => panic!("Unable to read config"),
    };

    simple_logger::init_with_level(config.get_log_level()).unwrap();

    // Get web server address from config
    let server_address = config.service.rust_address.clone();
    // Setup web server data
    let (tx_rest, rx_rest) = mpsc::channel();

    let app_state = AppState {
        msg_from_rest_api: tx_rest,
    };
    let web_state = web::Data::new(app_state);

    // Setup logic
    let mut logic = Logic::new(&config);
    logic.setup();

    // Used to track peer connection threads
    let mut children = ThreadTracker::new();
    let mut manager = ThreadManager::new(rx_rest);
    let tx = manager.get_tx();

    // Decode config
    let ips: Vec<IpAddr> = config
        .get_ips()
        .expect("Error decoding config ip addresses");

    // Start the peer threads
    let handle = thread::spawn(move ||
        // Cycle around all the IP addresses
        for ip in ips.into_iter().cycle() {
            manager.create_thread(ip, &mut children, &config);
            if manager.process_messages(&mut children, &mut logic) {
                break;
            };
        }
    );

    // Start webserver
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web_state.clone())
            .service(broadcast_tx)
            .service(version)
            .service(add_monitor)
            .service(delete_monitor)
    })
    .workers(1)
    .bind(server_address)
    .unwrap();
    server.run().await.unwrap();

    // Handle control C
    let mut signals = Signals::new([SIGINT]).unwrap();
    // create a stop message

    thread::spawn(move || {
        for sig in signals.forever() {
            if sig == SIGINT {
                log::info!("Someone tried to kill us... (Please wait I am still processing)");
                let stop_msg = PeerEventMessage {
                    time: time::SystemTime::now(),
                    peer: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                    event: PeerEventType::Stop,
                };
                tx.send(stop_msg).unwrap();
            }
        }
    });

    // wait for peer threads
    handle.join().unwrap();
}
