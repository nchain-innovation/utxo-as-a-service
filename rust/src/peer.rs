use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use std::thread;
use std::time;

use sv::peer::{Peer, SVPeerFilter};

use sv::messages::{Version, NODE_BITCOIN_CASH, PROTOCOL_VERSION};
use sv::util::rx::Observable;
use sv::util::secs_since;

use crate::config::Config;
use crate::event_handler::{EventHandler, PeerEvent, RequestMessage};

pub fn connect_to_peer(
    ip: IpAddr,
    config: Config,
    tx: mpsc::Sender<PeerEvent>,
    rx: Arc<Mutex<mpsc::Receiver<RequestMessage>>>,
    running: Arc<AtomicBool>,
) {
    // Given the ip address and config connect to the peer, quit if timeout occurs
    let port = config.service.port;
    let network = config.get_network().expect("Error decoding config network");

    let version = Version {
        version: PROTOCOL_VERSION,
        services: NODE_BITCOIN_CASH,
        timestamp: secs_since(time::UNIX_EPOCH) as i64,
        user_agent: config.service.user_agent,
        relay: true, // This is required to receive Tx messages
        ..Default::default()
    };
    let peer = Peer::connect(ip, port, network, version, SVPeerFilter::new(0));

    // Setup Event handler
    let event_handler = Arc::new(EventHandler::new(tx, rx));
    peer.connected_event().subscribe(&event_handler);
    peer.disconnected_event().subscribe(&event_handler);
    peer.messages().subscribe(&event_handler);

    // Wait
    let one_second = time::Duration::from_secs(1);
    let two_seconds = time::Duration::from_secs(2);
    thread::sleep(one_second);
    while running.load(Ordering::Relaxed)
        && event_handler.get_elapsed_time() < config.service.timeout_period
    {
        let start = time::Instant::now();
        thread::sleep(one_second);
        // Check time here to see if we have been asleep
        if start.elapsed() > two_seconds {
            let asleep_time = start.elapsed().as_millis() as f64;
            println!("Have been asleep for {} seconds", asleep_time / 1000.0);
            // If so stop
            break;
        }
    }
    if event_handler.get_elapsed_time() >= config.service.timeout_period {
        println!("timed out at {} seconds", event_handler.get_elapsed_time());
    }
    peer.disconnect();
}
