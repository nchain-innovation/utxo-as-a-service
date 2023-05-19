use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use rand::Rng;
use std::thread;
use std::time;

use chain_gang::peer::{Peer, SVPeerFilter};

use chain_gang::messages::{Version, NODE_BITCOIN_CASH, PROTOCOL_VERSION};
use chain_gang::util::rx::Observable;
use chain_gang::util::secs_since;

use crate::config::Config;
use crate::event_handler::EventHandler;
use crate::peer_event::PeerEventMessage;

pub struct PeerConnection {
    pub peer: Arc<Peer>,
    pub event_handler: Arc<EventHandler>,
}

impl PeerConnection {
    pub fn new(ip: IpAddr, config: &Config, tx: mpsc::Sender<PeerEventMessage>) -> Self {
        let port = config.get_network_settings().port;
        let network = config.get_network().expect("Error decoding config network");
        let user_agent = &config.service.user_agent;

        let mut rng = rand::thread_rng();

        let version = Version {
            version: PROTOCOL_VERSION,
            services: NODE_BITCOIN_CASH,
            timestamp: secs_since(time::UNIX_EPOCH) as i64,
            user_agent: user_agent.to_string(),
            relay: true, // This must be set to true to receive Tx messages
            nonce: rng.gen::<u64>(),
            start_height: 738839,
            ..Default::default()
        };

        let peer = Peer::connect(ip, port, network, version, SVPeerFilter::new(0));

        let event_handler = Arc::new(EventHandler::new(tx));
        peer.connected_event().subscribe(&event_handler);
        peer.disconnected_event().subscribe(&event_handler);
        peer.messages().subscribe(&event_handler);

        PeerConnection {
            peer,
            event_handler,
        }
    }

    pub fn wait_for_messages(&self, timeout_period: f64, running: Arc<AtomicBool>) {
        // Wait
        let one_second = time::Duration::from_secs(1);
        let two_seconds = time::Duration::from_secs(2);
        thread::sleep(one_second);
        while running.load(Ordering::Relaxed)
            && self.event_handler.get_elapsed_time() < timeout_period
        {
            let start = time::Instant::now();
            thread::sleep(one_second);
            // Check time here to see if we have been asleep
            if start.elapsed() > two_seconds {
                // let the event_handler know we have been asleep
                self.event_handler.set_connected(false);

                let asleep_time = start.elapsed().as_millis() as f64;
                println!("Have been asleep for {} seconds", asleep_time / 1000.0);
                // If so stop
                break;
            }
        }
        if self.event_handler.get_elapsed_time() >= timeout_period {
            println!(
                "timed out at {} seconds",
                self.event_handler.get_elapsed_time()
            );
        }
        self.peer.disconnect();
    }
}
