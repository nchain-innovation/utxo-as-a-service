use std::time;
use std::net::IpAddr;
use std::thread;


use sv::peer::{Peer, PeerConnected, PeerDisconnected, PeerMessage, SVPeerFilter};
use std::sync::{Arc, Mutex};

use sv::messages::{Addr, Inv, Message, Version, NODE_BITCOIN_CASH, PROTOCOL_VERSION};
use sv::util::rx::{Observable, Observer};
use sv::util::secs_since;


use crate::services::decode_services;
use crate::config::Config;


struct EventHandler {
    last_event: Mutex<time::Instant>,
}

impl EventHandler {
    fn new() -> Self {
        EventHandler {
            last_event: Mutex::new(time::Instant::now()),
        }
    }

    fn get_elapsed_time(&self) -> f64 {
        let x = self.last_event.lock().unwrap();
        x.elapsed().as_secs_f64()
    }

    fn update_timer(&self) {
        // this is called whenever a event is received
        let mut x = self.last_event.lock().unwrap();
        *x = time::Instant::now();
    }
}

impl Observer<PeerConnected> for EventHandler {
    fn next(&self, event: &PeerConnected) {
        // On connected
        self.update_timer();
        let sys_time = time::SystemTime::now()
            .duration_since(time::SystemTime::UNIX_EPOCH)
            .unwrap();
        let msg_start = format!("{:?}, {}:{},", sys_time, event.peer.ip, event.peer.port);
        // Handle node connected
        println!("{} Connected", msg_start);
    }
}

impl Observer<PeerDisconnected> for EventHandler {
    fn next(&self, event: &PeerDisconnected) {
        // On disconnected
        self.update_timer();

        // Handle node disconnected
        let sys_time = time::SystemTime::now()
            .duration_since(time::SystemTime::UNIX_EPOCH)
            .unwrap();
        let msg_start = format!("{:?}, {}:{},", sys_time, event.peer.ip, event.peer.port);
        println!("{} Disconnected", msg_start);
    }
}

// Message handlers
fn on_addr(addr: &Addr, peer: &Arc<Peer>) {
    // On address message
    let sys_time = time::SystemTime::now()
        .duration_since(time::SystemTime::UNIX_EPOCH)
        .unwrap();
    let msg_start = format!("{:?}, {}:{},", sys_time, peer.ip, peer.port);

    for address in addr.addrs.iter() {
        println!("{} addr={}", msg_start, address.addr.ip);
    }

    let version = peer.version().expect("failed to get version!");
    println!("version={:?}", version);
    println!(
        "user_agent={}, services={} ({:?})",
        version.user_agent,
        version.tx_addr.services,
        decode_services(version.tx_addr.services)
    );
}

fn on_inv(inv: &Inv, peer: &Arc<Peer>) {
    // On inv message
    let sys_time = time::SystemTime::now()
        .duration_since(time::SystemTime::UNIX_EPOCH)
        .unwrap();
    let msg_start = format!("{:?}, {}:{},", sys_time, peer.ip, peer.port);

    for i in inv.objects.iter() {
        match i.obj_type {
            0 => println!("{} error", msg_start),
            1 => println!("{} tx={:?}", msg_start, i.hash),
            2 => println!("{} block={:?}", msg_start, i.hash),
            3 => println!("{} filtered block={:?}", msg_start, i.hash),
            4 => println!("{} compact block={:?}", msg_start, i.hash),
            x => println!("{} unknown={}", msg_start, x),
        }
    }
}

impl Observer<PeerMessage> for EventHandler {
    fn next(&self, event: &PeerMessage) {
        // On peer message, decode it and call the message handler
        // Note that the framework already handles the handshake (including the version number)
        // and ping, feefilter, sendcmpt  sendheaders messages
        self.update_timer();

        match &event.message {
            Message::Addr(addr) => on_addr(addr, &event.peer),
            Message::Inv(inv) => on_inv(inv, &event.peer),
            _msg => {
                // println!("default {:?}", msg)
            }
        }
    }
}

pub fn connect_to_peer(ip: IpAddr, config: Config) {
    // Given the ip address and config connect to the peer, quit if timeout occurs
    let port = config.port;
    let network = config.get_network().expect("Error decoding config network");

    let version = Version {
        version: PROTOCOL_VERSION,
        services: NODE_BITCOIN_CASH,
        timestamp: secs_since(time::UNIX_EPOCH) as i64,
        user_agent: config.user_agent,
        relay: true, // This is required for Tx messages
        ..Default::default()
    };

    let peer = Peer::connect(ip, port, network, version, SVPeerFilter::new(0));

    // Setup Event handler
    let event_handler = Arc::new(EventHandler::new());
    peer.connected_event().subscribe(&event_handler);
    peer.disconnected_event().subscribe(&event_handler);
    peer.messages().subscribe(&event_handler);

    // Wait
    let one_second = time::Duration::from_secs(1);
    thread::sleep(one_second);
    while event_handler.get_elapsed_time() < config.timeout_period {
        thread::sleep(one_second);
    }
    peer.disconnect();
}
