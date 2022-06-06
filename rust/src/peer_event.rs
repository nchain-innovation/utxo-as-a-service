use std::fmt;
use std::net::IpAddr;
use std::time;

use sv::messages::{Addr, Block, Headers, Tx};

use crate::uaas::util::timestamp_as_string;


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
            EventType::Block(block) => write!(
                f,
                "Block={:?} - {}",
                block.header.hash(),
                timestamp_as_string(block.header.timestamp)
            ),
            EventType::Headers(headers) => write!(f, "Headers={:?}", headers.headers.len()),
        }
    }
}

// PeerEvents - used for sending messages from peer threads to main thread
pub struct PeerEvent {
    pub time: time::SystemTime,
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
