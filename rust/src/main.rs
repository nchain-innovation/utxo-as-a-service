#[macro_use]
extern crate lazy_static;
extern crate chrono;
extern crate hex;
extern crate rand;
extern crate regex;

use std::thread;

mod config;
mod connect_to_peer;
mod event_handler;
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

/*
fn message_processor(
    children: &mut ThreadTracker,
    logic: &mut Logic,
    rx: &mpsc::Receiver<PeerEvent>,
    request_tx: &mpsc::Sender<RequestMessage>,
) {
    //for received in rx {
    while let Ok(received) = rx.recv() {
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
                logic.set_state(ServerStateType::Disconnected);
                // Wait for thread, sets state to Finished
                println!("join thread");
                children.stop(&received.peer);
                children.join_thread(&received.peer);
                children.print();
                if children.all_finished() {
                    println!("all finished");
                    break;
                }
            }

            EventType::Tx(tx) => logic.on_tx(tx),
            EventType::Block(block) => logic.on_block(block),
            EventType::Addr(addr) => logic.on_addr(addr),
            EventType::Headers(headers) => logic.on_headers(headers),
        }

        if let Some(msg) = logic.message_to_send() {
            // request a block
            request_tx.send(msg).unwrap();
        }
    }
}
*/

fn main() {
    let count = thread::available_parallelism().expect("parallel error");
    println!("Available parallelism = {}", count);

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

    manager.create_threads(&mut children, config);
    manager.process_messages(&mut children, &mut logic);

    /*

    // Set up channels
    // Used to send messages from child to main
    let (tx, rx) = mpsc::channel();
    // Used to send messages from child to main
    let (request_tx, request_rx) = mpsc::channel();
    let wrapped_request_rx = Arc::new(Mutex::new(request_rx));

    // Used to track peer connection threads
    let mut children = ThreadTracker::new();

    // Start the threads
    for ip in ips.into_iter() {
        let local_config = config.clone();
        let local_tx = tx.clone();
        let local_rx = wrapped_request_rx.clone();
        let local_running: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));
        let peer_running = local_running.clone();

        let peer = PeerThread {
            thread: Some(thread::spawn(move || {
                connect_to_peer(ip, local_config, local_tx, local_rx, local_running)
            })),
            status: PeerThreadStatus::Started,
            running: peer_running,
            started_at: Instant::now(),
        };
        children.add(ip, peer);
    }

    // Process messages
    message_processor(&mut children, &mut logic, &rx, &request_tx);

    */
}
