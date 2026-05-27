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
        value => format!("unknown obj_type {value}"),
    }
}

impl fmt::Display for PeerEventType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PeerEventType::Connected(detail) => write!(f, "Connected=({detail})"),
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
                0 => write!(f, "Inv=0"),
                1 => {
                    let hash = Hash256::encode(&inv.objects[0].hash);
                    write!(
                        f,
                        "Inv=1 ({}) {hash}",
                        obj_type_as_string(inv.objects[0].obj_type)
                    )
                }
                len => write!(f, "Inv={len}"),
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
        let elapsed = self
            .time
            .duration_since(time::SystemTime::UNIX_EPOCH)
            .map(|duration| format!("{duration:?}"))
            .unwrap_or_else(|_| "unknown".to_string());
        write!(f, "{elapsed}, {}, {}", self.peer, self.event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chain_gang::messages::InvVect;

    #[test]
    fn inv_display_handles_empty_inventory() {
        let event = PeerEventType::Inv(Inv { objects: vec![] });
        assert_eq!(format!("{event}"), "Inv=0");
    }

    #[test]
    fn inv_display_handles_multiple_objects() {
        let event = PeerEventType::Inv(Inv {
            objects: vec![
                InvVect {
                    obj_type: 1,
                    hash: Hash256::default(),
                },
                InvVect {
                    obj_type: 2,
                    hash: Hash256::default(),
                },
            ],
        });
        assert_eq!(format!("{event}"), "Inv=2");
    }

    #[test]
    fn rel01_stop_event_is_used_for_shutdown() {
        assert!(matches!(PeerEventType::Stop, PeerEventType::Stop));
    }
}
