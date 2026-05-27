#[macro_use]
extern crate lazy_static;

use actix_web::{web, App, HttpServer};
use std::{
    net::{IpAddr, Ipv4Addr},
    panic, process,
    sync::mpsc,
    thread, time,
};
use tokio::signal;

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
mod thread_util;
mod uaas;

use crate::{
    config::get_config,
    peer_event::{PeerEventMessage, PeerEventType},
    rest_api::{add_monitor, broadcast_tx, delete_monitor, health, version, AppState},
    thread_manager::ThreadManager,
    thread_tracker::ThreadTracker,
    thread_util::catch_unwind_logged,
    uaas::logic::Logic,
};

#[actix_web::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("Fatal startup error: {err}");
        process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    // Log panics without terminating unrelated threads (for example the web server).
    let orig_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        log::error!("Thread panic: {panic_info}");
        orig_hook(panic_info);
    }));

    let config = get_config("UAASR_CONFIG", "../data/uaasr.toml")?;

    simple_logger::init_with_level(config.get_log_level())
        .map_err(|err| format!("failed to initialize logger: {err}"))?;

    config.validate_startup()?;

    // Get web server address from config
    let server_address = config.service.rust_address.clone();
    // Setup web server data
    let (tx_rest, rx_rest) = mpsc::channel();

    let app_state = AppState {
        msg_from_rest_api: tx_rest,
        api_key: config.web_interface.api_key.clone(),
    };
    let web_state = web::Data::new(app_state);

    // Setup logic
    let mut logic = Logic::new(&config)?;
    logic.setup();

    // Used to track peer connection threads
    let mut children = ThreadTracker::new();
    let mut manager = ThreadManager::new(rx_rest);
    let tx = manager.get_tx();

    let ips = config.get_ips()?;

    // Start the peer threads
    let handle = thread::spawn(move || {
        catch_unwind_logged("peer manager", || {
            for ip in ips.into_iter().cycle() {
                manager.create_thread(ip, &mut children, &config);
                if manager.process_messages(&mut children, &mut logic) {
                    break;
                }
            }
        });
    });

    // Start webserver
    let server = HttpServer::new(move || {
        App::new()
            .app_data(web_state.clone())
            .service(health)
            .service(broadcast_tx)
            .service(version)
            .service(add_monitor)
            .service(delete_monitor)
    })
    .workers(1)
    .bind(&server_address)
    .map_err(|err| format!("failed to bind web server to {server_address}: {err}"))?
    .run();

    let server_handle = server.handle();
    let tx_stop = tx.clone();

    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        log::info!("Shutdown requested, stopping peer threads and web server...");
        let stop_msg = PeerEventMessage {
            time: time::SystemTime::now(),
            peer: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            event: PeerEventType::Stop,
        };
        if tx_stop.send(stop_msg).is_err() {
            log::warn!("Failed to send stop message to peer manager");
        }
        server_handle.stop(true).await;
    });

    server
        .await
        .map_err(|err| format!("web server error: {err}"))?;

    // Wait for peer threads
    if handle.join().is_err() {
        log::error!("Peer manager thread panicked during shutdown");
    }

    Ok(())
}

async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = signal::ctrl_c().await {
            log::error!("failed to install Ctrl+C handler: {err}");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(err) => {
                log::error!("failed to install SIGTERM handler: {err}");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
