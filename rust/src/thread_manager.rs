use std::{
    net::IpAddr,
    sync::{atomic::AtomicBool, mpsc, Arc},
    thread,
    time::{Duration, Instant},
};

use chain_gang::messages::Message;

use crate::{
    config::Config,
    peer_connection::PeerConnection,
    peer_event::{PeerEventMessage, PeerEventType},
    peer_thread::{PeerThread, PeerThreadStatus},
    rest_api::RestEventMessage,
    thread_tracker::ThreadTracker,
    uaas::logic::{Logic, ServerStateType},
};

pub struct ThreadManager {
    rx_peer: mpsc::Receiver<PeerEventMessage>,
    tx_peer: mpsc::Sender<PeerEventMessage>,
    rx_rest: mpsc::Receiver<RestEventMessage>,
}

impl ThreadManager {
    pub fn new(rx_rest: mpsc::Receiver<RestEventMessage>) -> Self {
        // Used to send messages from PeerConnection(s) to ThreadManager
        let (tx_peer, rx_peer) = mpsc::channel();
        ThreadManager {
            rx_peer,
            tx_peer,
            rx_rest,
        }
    }

    pub fn get_tx(&self) -> mpsc::Sender<PeerEventMessage> {
        self.tx_peer.clone()
    }
    pub fn create_thread(
        &mut self,
        ip: IpAddr,
        thread_tracker: &mut ThreadTracker,
        config: &Config,
    ) {
        let local_config = config.clone();
        let local_tx = self.tx_peer.clone();

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
        log::info!("{}", received);
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
                log::debug!("join thread");
                thread_tracker.stop(&received.peer);
                thread_tracker.join_thread(&received.peer);
                thread_tracker.print();
                if thread_tracker.all_finished() {
                    log::debug!("all finished");
                    return false;
                }
            }

            PeerEventType::Tx(tx) => logic.on_tx(tx),
            PeerEventType::Block(block) => logic.on_block(block),
            PeerEventType::Addr(addr) => logic.on_addr(addr),
            PeerEventType::Headers(headers) => logic.on_headers(headers),
            PeerEventType::Inv(inv) => logic.on_inv(inv),

            PeerEventType::Stop => {
                log::info!("Stop");
                thread_tracker.stop_all();
                return false;
            }
        }
        true
    }

    pub fn process_messages(
        &mut self,
        thread_tracker: &mut ThreadTracker,
        logic: &mut Logic,
    ) -> bool {
        let mut keep_looping = true;
        let mut should_stop: bool = false;
        let timeout_period = Duration::from_millis(100);

        while keep_looping {
            if let Ok(received) = self.rx_peer.recv_timeout(timeout_period) {
                should_stop = received.event == PeerEventType::Stop;
                // Process the event
                keep_looping = self.process_event(received.clone(), thread_tracker, logic);
                // Check to see if logic has a message or more to send
                logic.message_to_send().iter().for_each(|msg| {
                    if let Some(peer) = thread_tracker.get_connected_peer() {
                        match peer.send(msg) {
                            Ok(_) => {}
                            Err(e) => log::warn!("error sending message {:?}", e),
                        };
                    }
                });
            }
            if let Ok(event) = self.rx_rest.try_recv() {
                log::info!("{:?}", &event);

                match event {
                    RestEventMessage::TxForBroadcast(tx) => {
                        if logic.tx_exists(tx.hash()) {
                            log::info!("Broadcast Tx already exists {}", &tx.hash().encode());
                            continue;
                        }
                        if let Some(peer) = thread_tracker.get_connected_peer() {
                            let message = Message::Tx(tx.clone());
                            peer.send(&message).unwrap();
                            logic.on_tx(tx);
                            logic.flush_database_cache();
                        }
                    }
                    RestEventMessage::AddMonitor(monitor) => logic.tx_analyser.add_monitor(monitor),
                    RestEventMessage::DeleteMonitor(monitor_name) => {
                        logic.tx_analyser.delete_monitor(&monitor_name)
                    }
                }
            }
        }

        // Return true if should quit
        should_stop
    }
}
