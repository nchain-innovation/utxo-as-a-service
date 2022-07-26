use std::sync::mpsc;
use std::time;

use std::sync::{Arc, Mutex};
use sv::messages::{Addr, Block, FeeFilter, Headers, Inv, InvVect, Message, SendCmpct, Tx};

use sv::peer::{Peer, PeerConnected, PeerDisconnected, PeerMessage};
use sv::util::rx::Observer;

use crate::peer_event::{PeerEventMessage, PeerEventType};
use crate::services::decode_services;

// Constants for inv messages
const TX: u32 = 1;
const BLOCK: u32 = 2;

// Event handler - processes peer events
pub struct EventHandler {
    last_event: Mutex<time::Instant>,
    mutex_tx: Mutex<mpsc::Sender<PeerEventMessage>>,
    connected_to_peer: Mutex<bool>,
}

impl EventHandler {
    pub fn new(tx: mpsc::Sender<PeerEventMessage>) -> Self {
        EventHandler {
            last_event: Mutex::new(time::Instant::now()),
            mutex_tx: Mutex::new(tx),
            connected_to_peer: Mutex::new(false),
         }
    }

    pub fn get_elapsed_time(&self) -> f64 {
        // Return how much time has passed since last message
        let x = self.last_event.lock().unwrap();
        x.elapsed().as_secs_f64()
    }

    fn set_connected(&self, connected: bool) {
        let mut connected_to_peer = self.connected_to_peer.lock().unwrap();
        *connected_to_peer = connected;
    }

    fn get_connected(&self) -> bool {
        let connected_to_peer = self.connected_to_peer.lock().unwrap();
        *connected_to_peer
    }

    fn update_timer(&self) {
        // Update the last message event timer, this is called whenever a event is received
        let mut x = self.last_event.lock().unwrap();
        *x = time::Instant::now();
    }

    fn send_msg(&self, msg: PeerEventMessage) {
        let tx = self.mutex_tx.lock().unwrap();
        tx.send(msg).unwrap()
    }

    // Message handlers
    fn on_addr(&self, addr: &Addr, peer: &Arc<Peer>) {
        // On addr message
        //for address in addr.addrs.iter() {
        let msg = PeerEventMessage {
            time: time::SystemTime::now(),
            peer: peer.ip,
            event: PeerEventType::Addr(addr.clone()),
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
            if self.get_connected() {
                peer.send(&want).unwrap();
            }
        }
    }

    fn on_block(&self, block: &Block, peer: &Arc<Peer>) {
        // println!("on_block {:?}", block);
        let msg = PeerEventMessage {
            time: time::SystemTime::now(),
            peer: peer.ip,
            event: PeerEventType::Block(block.clone()),
        };
        self.send_msg(msg);
    }

    fn on_tx(&self, tx: &Tx, peer: &Arc<Peer>) {
        // println!("on_tx {:?}", tx);
        let msg = PeerEventMessage {
            time: time::SystemTime::now(),
            peer: peer.ip,
            event: PeerEventType::Tx(tx.clone()),
        };
        self.send_msg(msg);
    }

    fn on_headers(&self, headers: &Headers, peer: &Arc<Peer>) {
        // println!("on_on_headers {:?}", headers);
        let msg = PeerEventMessage {
            time: time::SystemTime::now(),
            peer: peer.ip,
            event: PeerEventType::Headers(headers.clone()),
        };
        self.send_msg(msg);
    }

    fn on_feefilter(&self, value: &FeeFilter, peer: &Arc<Peer>) {
        println!("on_feefilter {:?}", value);

        let p = FeeFilter { minfee: 0 };
        let m = Message::FeeFilter(p);
        if self.get_connected() {
            peer.send(&m).unwrap();
        }
    }

    fn on_sendcmpct(&self, data: &SendCmpct, peer: &Arc<Peer>) {
        println!("on_sendcmpct {:?}", data);
        let p = SendCmpct {
            enable: 0,
            version: 1,
        };
        let m = Message::SendCmpct(p);
        if self.get_connected() {
            peer.send(&m).unwrap();
        }
    }
}

impl Observer<PeerConnected> for EventHandler {
    fn next(&self, event: &PeerConnected) {
        // On connected
        self.update_timer();

        let version = event.peer.version().expect("failed to get version!");
        self.set_connected(true);

        // dbg!(&version);
        let detail = format!(
            "user_agent={}, services={:x} ({:?})",
            version.user_agent,
            version.tx_addr.services,
            decode_services(version.tx_addr.services)
        );

        let msg = PeerEventMessage {
            time: time::SystemTime::now(),
            peer: event.peer.ip,
            event: PeerEventType::Connected(detail),
        };
        self.send_msg(msg);
    }
}

impl Observer<PeerDisconnected> for EventHandler {
    fn next(&self, event: &PeerDisconnected) {
        // On disconnected
        self.update_timer();
        self.set_connected(false);
        let msg = PeerEventMessage {
            time: time::SystemTime::now(),
            peer: event.peer.ip,
            event: PeerEventType::Disconnected,
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
            Message::FeeFilter(value) => self.on_feefilter(value, &event.peer),
            Message::SendCmpct(data) => self.on_sendcmpct(data, &event.peer),
            _msg => {
                // println!("default {:?}", _msg)
            }
        }
    }
}
