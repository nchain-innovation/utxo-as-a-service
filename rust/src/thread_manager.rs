use std::net::IpAddr;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;

use std::thread;
use std::time::Instant;

use crate::config::Config;
use crate::peer_connection::PeerConnection;
use crate::peer_event::{EventType, PeerEvent};
use crate::peer_thread::{PeerThread, PeerThreadStatus};
use crate::thread_tracker::ThreadTracker;
use crate::uaas::logic::{Logic, ServerStateType};

pub struct ThreadManager {
    rx: mpsc::Receiver<PeerEvent>,
    tx: mpsc::Sender<PeerEvent>,
}

impl ThreadManager {
    pub fn new() -> Self {
        // Used to send messages from child to main
        let (tx, rx) = mpsc::channel();
        ThreadManager { rx, tx }
    }

    pub fn create_thread(
        &mut self,
        ip: IpAddr,
        thread_tracker: &mut ThreadTracker,
        config: &Config,
    ) {
        let local_config = config.clone();
        let local_tx = self.tx.clone();

        // Used to send messages from main to child PeerConnection
        let (request_tx, request_rx) = mpsc::channel();

        let local_running: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));
        let peer_running = local_running.clone();

        // Read config
        let timeout_period = config.get_network_settings().timeout_period;

        let peer = PeerThread {
            thread: Some(thread::spawn(move || {
                let peer = PeerConnection::new(ip, &local_config, local_tx, request_rx);
                peer.wait_for_messages(timeout_period, local_running);
            })),
            status: PeerThreadStatus::Started,
            running: peer_running,
            started_at: Instant::now(),
            request_tx,
        };
        thread_tracker.add(ip, peer);
    }

    pub fn process_messages(&mut self, thread_tracker: &mut ThreadTracker, logic: &mut Logic) {
        //for received in rx {
        while let Ok(received) = self.rx.recv() {
            println!("{}", received);
            match received.event {
                EventType::Connected(_) => {
                    thread_tracker.set_status(&received.peer, PeerThreadStatus::Connected);
                    thread_tracker.print();
                    logic.set_state(ServerStateType::Connected);
                }

                EventType::Disconnected => {
                    // If we have disconnected then there is the opportunity to start another thread
                    thread_tracker.set_status(&received.peer, PeerThreadStatus::Disconnected);
                    logic.set_state(ServerStateType::Disconnected);
                    // Wait for thread, sets state to Finished
                    println!("join thread");
                    thread_tracker.stop(&received.peer);
                    thread_tracker.join_thread(&received.peer);
                    thread_tracker.print();
                    if thread_tracker.all_finished() {
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
                if let Some(request_tx) = thread_tracker.get_request_tx(&received.peer) {
                    request_tx.send(msg).unwrap();
                }
            }
        }
    }
}
