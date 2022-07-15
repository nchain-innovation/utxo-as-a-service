use std::net::IpAddr;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;

use std::thread;
use std::time::{Duration, Instant};

use sv::messages::{BlockLocator, Message};
use sv::util::Hash256;

use crate::config::Config;
use crate::peer_connection::PeerConnection;
use crate::peer_event::{PeerEventMessage, PeerEventType};
use crate::peer_thread::{PeerThread, PeerThreadStatus};
use crate::rest_api::AppState;
use crate::thread_tracker::ThreadTracker;
use crate::uaas::logic::{Logic, ServerStateType};
use actix_web::web;

pub struct ThreadManager {
    rx: mpsc::Receiver<PeerEventMessage>,
    tx: mpsc::Sender<PeerEventMessage>,
}

impl ThreadManager {
    pub fn new() -> Self {
        // Used to send messages from PeerConnection(s) to ThreadManager
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

        let local_running: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));
        let peer_running = local_running.clone();

        // Read config
        let timeout_period = config.get_network_settings().timeout_period;

        let peer_connection = PeerConnection::new(ip, &local_config, local_tx);
        let peer = peer_connection.peer.clone();
        let peer_thread = PeerThread {
            thread: Some(thread::spawn(move || {
                peer_connection.wait_for_messages(timeout_period, local_running);
            })),
            status: PeerThreadStatus::Started,
            running: peer_running,
            started_at: Instant::now(),
            peer: Some(peer),
        };
        thread_tracker.add(ip, peer_thread);
    }

    fn process_event(
        &self,
        received: PeerEventMessage,
        thread_tracker: &mut ThreadTracker,
        logic: &mut Logic,
    ) -> bool {
        // Return false if enclosing loop should finish
        println!("{}", received);
        match received.event {
            PeerEventType::Connected(_) => {
                thread_tracker.set_status(&received.peer, PeerThreadStatus::Connected);
                thread_tracker.print();
                logic.set_state(ServerStateType::Connected);
                logic.connection.on_connect(&received.peer);
            }

            PeerEventType::Disconnected => {
                // If we have disconnected then there is the opportunity to start another thread
                thread_tracker.set_status(&received.peer, PeerThreadStatus::Disconnected);
                logic.set_state(ServerStateType::Disconnected);
                logic.connection.on_disconnect(&received.peer);
                // Wait for thread, sets state to Finished
                println!("join thread");
                thread_tracker.stop(&received.peer);
                thread_tracker.join_thread(&received.peer);
                thread_tracker.print();
                if thread_tracker.all_finished() {
                    println!("all finished");
                    return false;
                }
            }

            PeerEventType::Tx(tx) => logic.on_tx(tx),
            PeerEventType::Block(block) => logic.on_block(block),
            PeerEventType::Addr(addr) => logic.on_addr(addr),
            PeerEventType::Headers(headers) => logic.on_headers(headers),
        }
        true
    }

    pub fn process_messages(
        &mut self,
        thread_tracker: &mut ThreadTracker,
        logic: &mut Logic,
        data: &web::Data<AppState>,
    ) {
        let recv_duration = Duration::from_millis(500);
        let mut keep_looping = true;

        while keep_looping {
            let r = self.rx.recv_timeout(recv_duration);

            if let Ok(received) = r {
                // Process the event
                keep_looping = self.process_event(received.clone(), thread_tracker, logic);

                // Check to see if logic has something to send
                if let Some(value) = logic.message_to_send() {
                    // Request a block
                    if let Some(peer) = thread_tracker.get_connected_peer() {
                        // Build message
                        let mut locator = BlockLocator::default();
                        let hash = Hash256::decode(&value).unwrap();
                        locator.block_locator_hashes.push(hash);
                        let message = Message::GetBlocks(locator);
                        peer.send(&message).unwrap();
                    }
                }
            }

            // Check to see if any tx to broadcast
            let mut txs_for_broadcast = data.txs_for_broadcast.lock().unwrap();
            while let Some(tx) = txs_for_broadcast.pop() {
                dbg!(&tx);
                if let Some(peer) = thread_tracker.get_connected_peer() {
                    let message = Message::Tx(tx.clone());
                    peer.send(&message).unwrap();
                }
            }
        }
    }
}
