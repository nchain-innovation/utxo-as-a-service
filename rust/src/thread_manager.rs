use std::net::IpAddr;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;

use std::thread;
use std::time::Instant;

use crate::config::Config;
use crate::connect_to_peer::connect_to_peer;
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

    pub fn create_threads(&mut self, thread_tracker: &mut ThreadTracker, config: Config) {
        // Decode config
        let ips: Vec<IpAddr> = config
            .get_ips()
            .expect("Error decoding config ip addresses");
        /*
        // Create the peer listening threads
        for ip in ips.into_iter() {
            let mut peer = PeerThread::new();
            peer.connect(ip, config, self.tx.clone(), self.wrapped_request_rx.clone());
            thread_tracker.add(ip, peer);
        }
        */

        for ip in ips.into_iter() {
            let local_config = config.clone();
            let local_tx = self.tx.clone();
            //let local_rx = self.request_rx.clone();
            //let local_rx = Arc::new(Mutex::new(self.request_rx));
            // Used to send messages from main to child
            let (request_tx, request_rx) = mpsc::channel();

            let local_running: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));
            let peer_running = local_running.clone();

            let peer = PeerThread {
                thread: Some(thread::spawn(move || {
                    connect_to_peer(ip, local_config, local_tx, request_rx, local_running)
                })),
                status: PeerThreadStatus::Started,
                running: peer_running,
                started_at: Instant::now(),
                request_tx,
            };
            thread_tracker.add(ip, peer);
        }
    }

    pub fn process_messages(&mut self, thread_tracker: &mut ThreadTracker, logic: &mut Logic) {
        //rx: &mpsc::Receiver<PeerEvent>,
        //request_tx: &mpsc::Sender<RequestMessage>,
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

    /*
    pub fn process_messages(&mut self, thread_tracker: &mut ThreadTracker, conn: &mut PooledConn) {

        // Process messages
        for received in &self.rx {
            println!("{}", received);
            match received.event {
                EventType::Connected(ref detail) => {
                    thread_tracker.set_status(&received.peer, PeerThreadStatus::Connected);
                    conn.exec_drop(&connect_insert,
                        params! { "date" => current_time_as_string(), "time" => received.get_time(), "ip" => received.get_ip(), "event" => "connected", "detail" => detail} ).unwrap();

                    thread_tracker.print();
                }

                EventType::Disconnected => {
                    // If we have disconnected then there is the opportunity to start another thread
                    if let Some(peer_thread) = thread_tracker.get_thread(&received.peer) {
                        let pthread = peer_thread.wait_for_join();
                        thread_tracker.add(received.peer, pthread);
                    }

                    thread_tracker.set_status(&received.peer, PeerThreadStatus::Disconnected);
                    conn.exec_drop(&connect_insert,
                        params! { "date" => current_time_as_string(), "time" => received.get_time(), "ip" => received.get_ip(), "event" => "disconnected", "detail" => ""} ).unwrap();
                    thread_tracker.set_status(&received.peer, PeerThreadStatus::Waiting);
                }

                EventType::Timeout => {
                    // Just record the fact that we have received a timeout message
                    conn.exec_drop(&connect_insert,
                        params! { "date" => current_time_as_string(), "time" => received.get_time(), "ip" => received.get_ip(), "event" => "timeout", "detail" => ""} ).unwrap();
                }

                EventType::Tx(ref hash) => {
                    conn.exec_drop(&tx_insert,
                        params! { "time" => received.get_time(), "ip" => received.get_ip(), "tx" => hash} ).unwrap();
                }

                EventType::Block(ref hash) => {
                    conn.exec_drop(&block_insert,
                        params! { "time" => received.get_time(), "ip" => received.get_ip(), "block" => hash} ).unwrap();
                }

                EventType::Addr(ref detail) => {
                    conn.exec_drop(&addr_insert,
                        params! { "time" => received.get_time(), "ip" => received.get_ip(), "address" => detail} ).unwrap();
                }
            }

            thread_tracker.handle_waiting_threads();
        }
    }
    */
}
