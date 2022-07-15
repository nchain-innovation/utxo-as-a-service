use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use sv::peer::Peer;

// Used to track the threads
#[derive(Debug, PartialEq)]
pub enum PeerThreadStatus {
    Started,
    Connected,
    Disconnected,
    Finished,
}

#[derive(Debug)]
pub struct PeerThread {
    pub thread: Option<thread::JoinHandle<()>>,
    pub status: PeerThreadStatus,
    pub running: Arc<AtomicBool>,
    pub started_at: Instant,
    pub peer: Option<Arc<Peer>>,
}
