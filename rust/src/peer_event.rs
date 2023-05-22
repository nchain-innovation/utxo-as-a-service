use std::fmt;
use std::net::IpAddr;
use std::time;

use chain_gang::messages::{Addr, Block, Headers, Tx};

use crate::uaas::util::timestamp_as_string;

// EventsType - used to identify the type of event that is being sent to parent thread
#[derive(PartialEq, Clone, Eq)]
pub enum PeerEventType {
    Connected(String),
    Disconnected,
    Addr(Addr),
    Tx(Tx),
    Block(Block),
    Headers(Headers),
    Stop, // used to stop system
}

impl fmt::Display for PeerEventType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PeerEventType::Connected(detail) => write!(f, "Connected=({})", detail),
            PeerEventType::Disconnected => write!(f, "Disconnected"),
            PeerEventType::Addr(addr) => write!(f, "Addr={}", addr.addrs.len()),
            PeerEventType::Tx(tx) => write!(f, "Tx={:?}", tx.hash()),
            PeerEventType::Block(block) => write!(
                f,
                "Block={:?} - {}",
                block.header.hash(),
                timestamp_as_string(block.header.timestamp)
            ),
            PeerEventType::Headers(headers) => write!(f, "Headers={:?}", headers.headers.len()),
            PeerEventType::Stop => write!(f, "Stop"),
        }
    }
}

#[derive(Clone)]
// PeerEventMessages - used for sending messages from peer threads to main thread
pub struct PeerEventMessage {
    pub time: time::SystemTime,
    pub peer: IpAddr,
    pub event: PeerEventType,
}

impl fmt::Display for PeerEventMessage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let sys_time = self
            .time
            .duration_since(time::SystemTime::UNIX_EPOCH)
            .unwrap();
        write!(f, "{:?}, {}, {}", sys_time, self.peer, self.event)
    }
}
