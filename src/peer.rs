use std::fmt;
use std::net::IpAddr;
use std::sync::mpsc;
use std::thread;
use std::time;

use serde::{Deserialize, Serialize};

use std::sync::{Arc, Mutex};
use sv::peer::{Peer, PeerConnected, PeerDisconnected, PeerMessage, SVPeerFilter};

use sv::messages::{Addr, Inv, Message, Version, NODE_BITCOIN_CASH, PROTOCOL_VERSION};
use sv::util::rx::{Observable, Observer};
use sv::util::secs_since;

use crate::config::Config;
use crate::services::decode_services;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum EventType {
    Connected(String),
    Disconnected,
    Addr(String),
    Tx(String),
    Block(String),
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &*self {
            EventType::Connected(detail) => write!(f, "Connected=({})", detail),
            EventType::Disconnected => write!(f, "Disconnected"),
            EventType::Addr(detail) => write!(f, "Addr={}", detail),
            EventType::Tx(hash) => write!(f, "Tx={}", hash),
            EventType::Block(hash) => write!(f, "Block={}", hash),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
// PeerEvents - used for sending messages to main thread
pub struct PeerEvent {
    time: time::SystemTime,
    peer: IpAddr,
    event: EventType,
}

impl fmt::Display for PeerEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let sys_time = self
            .time
            .duration_since(time::SystemTime::UNIX_EPOCH)
            .unwrap();
        write!(f, "{:?}, {}, {}", sys_time, self.peer, self.event)
    }
}

// Event handler
struct EventHandler {
    last_event: Mutex<time::Instant>,
    tx_mutex: Mutex<mpsc::Sender<PeerEvent>>,
}

impl EventHandler {
    fn new(tx: mpsc::Sender<PeerEvent>) -> Self {
        EventHandler {
            last_event: Mutex::new(time::Instant::now()),
            tx_mutex: Mutex::new(tx),
        }
    }

    fn get_elapsed_time(&self) -> f64 {
        // Return how much time has passed since last message
        let x = self.last_event.lock().unwrap();
        x.elapsed().as_secs_f64()
    }

    fn update_timer(&self) {
        // Update the last message event timer, this is called whenever a event is received
        let mut x = self.last_event.lock().unwrap();
        *x = time::Instant::now();
    }

    fn send_msg(&self, msg: PeerEvent) {
        let tx = self.tx_mutex.lock().unwrap();
        tx.send(msg).unwrap()
    }

    fn on_addr(&self, addr: &Addr, peer: &Arc<Peer>) {
        for address in addr.addrs.iter() {
            //println!("{} addr={}", msg_start, address.addr.ip);
            let msg = PeerEvent {
                time: time::SystemTime::now(),
                peer: peer.ip,
                event: EventType::Addr(address.addr.ip.to_string()),
            };
            self.send_msg(msg);
        }
    }

    fn on_inv(&self, inv: &Inv, peer: &Arc<Peer>) {
        // On inv message
        for i in inv.objects.iter() {
            match i.obj_type {
                1 => {
                    // TX
                    let hash = format!("{:?}", i.hash);
                    let msg = PeerEvent {
                        time: time::SystemTime::now(),
                        peer: peer.ip,
                        event: EventType::Tx(hash),
                    };
                    self.send_msg(msg);
                }
                2 => {
                    // Block
                    let hash = format!("{:?}", i.hash);

                    let msg = PeerEvent {
                        time: time::SystemTime::now(),
                        peer: peer.ip,
                        event: EventType::Block(hash),
                    };
                    self.send_msg(msg);
                }
                _ => {}
            }
        }
    }
}

impl Observer<PeerConnected> for EventHandler {
    fn next(&self, event: &PeerConnected) {
        // On connected
        self.update_timer();

        let version = event.peer.version().expect("failed to get version!");
        let detail = format!(
            "user_agent={}, services={} ({:?})",
            version.user_agent,
            version.tx_addr.services,
            decode_services(version.tx_addr.services)
        );

        let msg = PeerEvent {
            time: time::SystemTime::now(),
            peer: event.peer.ip,
            event: EventType::Connected(detail),
        };
        self.send_msg(msg);
    }
}

impl Observer<PeerDisconnected> for EventHandler {
    fn next(&self, event: &PeerDisconnected) {
        // On disconnected
        self.update_timer();

        let msg = PeerEvent {
            time: time::SystemTime::now(),
            peer: event.peer.ip,
            event: EventType::Disconnected,
        };
        self.send_msg(msg);
    }
}

// Message handlers

impl Observer<PeerMessage> for EventHandler {
    fn next(&self, event: &PeerMessage) {
        // On peer message, decode it and call the message handler
        // Note that the framework already handles the handshake (including the version number)
        // and ping, feefilter, sendcmpt  sendheaders messages
        self.update_timer();

        match &event.message {
            Message::Addr(addr) => self.on_addr(addr, &event.peer),
            Message::Inv(inv) => self.on_inv(inv, &event.peer),
            _msg => {
                // println!("default {:?}", msg)
            }
        }
    }
}

pub fn connect_to_peer(ip: IpAddr, config: Config, tx: mpsc::Sender<PeerEvent>) {
    // Given the ip address and config connect to the peer, quit if timeout occurs
    let port = config.port;
    let network = config.get_network().expect("Error decoding config network");

    let version = Version {
        version: PROTOCOL_VERSION,
        services: NODE_BITCOIN_CASH,
        timestamp: secs_since(time::UNIX_EPOCH) as i64,
        user_agent: config.user_agent,
        relay: true, // This is required to receive Tx messages
        ..Default::default()
    };
    let peer = Peer::connect(ip, port, network, version, SVPeerFilter::new(0));

    // Setup Event handler
    let event_handler = Arc::new(EventHandler::new(tx));
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
