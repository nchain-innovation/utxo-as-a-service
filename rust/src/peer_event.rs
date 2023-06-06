use std::fmt;
use std::net::IpAddr;
use std::time;

use chain_gang::{
    messages::{Addr, Block, Headers, Inv, Tx},
    util::Hash256,
};

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
    Inv(Inv),
    Stop, // used to stop system
}

fn obj_type_as_string(o_type: u32) -> String {
    match o_type {
        1 => "TX".to_string(),
        2 => "BLOCK".to_string(),
        3 => "FILTERED_BLOCK".to_string(),
        4 => "CMPCT_BLOCK".to_string(),
        value => format!("unknown obj_type {}", value),
    }
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
            PeerEventType::Inv(inv) => match inv.objects.len() {
                1 => {
                    let hash = Hash256::encode(&inv.objects[0].hash);
                    write!(
                        f,
                        "Inv={:?} ({}) {}",
                        inv.objects.len(),
                        obj_type_as_string(inv.objects[0].obj_type),
                        hash
                    )
                }
                _ => write!(
                    f,
                    "Inv={:?} ({})",
                    inv.objects.len(),
                    obj_type_as_string(inv.objects[0].obj_type)
                ),
            },

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
