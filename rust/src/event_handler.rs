use std::fmt;
use std::net::IpAddr;
use std::sync::mpsc;
use std::time;

use std::sync::{Arc, Mutex};
use sv::messages::{Addr, Block, BlockLocator, Headers, Inv, InvVect, Message, Tx};
use sv::peer::{Peer, PeerConnected, PeerDisconnected, PeerMessage};
use sv::util::Hash256;

use crate::services::decode_services;
use sv::util::rx::Observer;

// Constants for inv messages
const TX: u32 = 1;
const BLOCK: u32 = 2;

// EventsType - used to identify the type of event that is being sent to parent thread
#[derive(PartialEq)]
pub enum EventType {
    Connected(String),
    Disconnected,
    Addr(Addr),
    Tx(Tx),
    Block(Block),
    Headers(Headers),
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &*self {
            EventType::Connected(detail) => write!(f, "Connected=({})", detail),
            EventType::Disconnected => write!(f, "Disconnected"),
            EventType::Addr(addr) => write!(f, "Addr={}", addr.addrs.len()),
            EventType::Tx(tx) => write!(f, "Tx={:?}", tx.hash()),
            EventType::Block(block) => write!(f, "Block={:?}", block.header.hash()),
            EventType::Headers(headers) => write!(f, "Headers={:?}", headers.headers.len()),
        }
    }
}

// PeerEvents - used for sending messages from peer threads to main thread
pub struct PeerEvent {
    time: time::SystemTime,
    pub peer: IpAddr,
    pub event: EventType,
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

// These are messages sent from the main thread to the peer threads
#[derive(Debug)]
pub enum RequestMessage {
    BlockRequest(String),
}

// Event handler - processes peer events
pub struct EventHandler {
    last_event: Mutex<time::Instant>,
    mutex_tx: Mutex<mpsc::Sender<PeerEvent>>,
    arc_mutex_rx: Arc<Mutex<mpsc::Receiver<RequestMessage>>>,
}

impl EventHandler {
    pub fn new(
        tx: mpsc::Sender<PeerEvent>,
        rx: Arc<Mutex<mpsc::Receiver<RequestMessage>>>,
    ) -> Self {
        EventHandler {
            last_event: Mutex::new(time::Instant::now()),
            mutex_tx: Mutex::new(tx),
            arc_mutex_rx: rx,
        }
    }

    pub fn get_elapsed_time(&self) -> f64 {
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
        let tx = self.mutex_tx.lock().unwrap();
        tx.send(msg).unwrap()
    }

    fn recv_msg(&self) -> Result<RequestMessage, mpsc::TryRecvError> {
        let rx = self.arc_mutex_rx.lock().unwrap();
        rx.try_recv()
    }

    // Message handlers
    fn on_addr(&self, addr: &Addr, peer: &Arc<Peer>) {
        // On addr message
        //for address in addr.addrs.iter() {
        let msg = PeerEvent {
            time: time::SystemTime::now(),
            peer: peer.ip,
            event: EventType::Addr(addr.clone()),
        };
        self.send_msg(msg);
    }

    fn on_inv(&self, inv: &Inv, peer: &Arc<Peer>) {
        // On inv message
        let mut objects: Vec<InvVect> = Vec::new();

        for i in inv.objects.iter() {
            match i.obj_type {
                TX | BLOCK => objects.push(i.clone()),
                // ignore all others
                _ => {}
            }
        }
        // Request the txs and blocks in the inv message
        if !objects.is_empty() {
            let want = Message::GetData(Inv { objects });
            peer.send(&want).unwrap();
        }
    }

    fn on_block(&self, block: &Block, peer: &Arc<Peer>) {
        // println!("on_block {:?}", block);
        let msg = PeerEvent {
            time: time::SystemTime::now(),
            peer: peer.ip,
            event: EventType::Block(block.clone()),
        };
        self.send_msg(msg);
    }

    fn on_tx(&self, tx: &Tx, peer: &Arc<Peer>) {
        // println!("on_tx {:?}", tx);
        let msg = PeerEvent {
            time: time::SystemTime::now(),
            peer: peer.ip,
            event: EventType::Tx(tx.clone()),
        };
        self.send_msg(msg);
    }

    fn on_headers(&self, headers: &Headers, peer: &Arc<Peer>) {
        // println!("on_on_headers {:?}", headers);
        let msg = PeerEvent {
            time: time::SystemTime::now(),
            peer: peer.ip,
            event: EventType::Headers(headers.clone()),
        };
        self.send_msg(msg);
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
            Message::Block(block) => self.on_block(block, &event.peer),
            Message::Tx(tx) => self.on_tx(tx, &event.peer),
            Message::Headers(headers) => self.on_headers(headers, &event.peer),

            _msg => {
                // println!("default {:?}", msg)
            }
        }

        // Check to see if we have received anything to send
        if let Ok(msg) = self.recv_msg() {
            match &msg {
                RequestMessage::BlockRequest(value) => {
                    // Build message
                    let mut locator = BlockLocator::default();
                    let hash = Hash256::decode(value).unwrap();

                    locator.block_locator_hashes.push(hash);
                    let message = Message::GetBlocks(locator);

                    event.peer.send(&message).unwrap();
                }
            }
        }
    }
}
