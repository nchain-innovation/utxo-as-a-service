use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use chain_gang::peer::Peer;

// Used to track the threads
#[derive(Debug, PartialEq, Eq)]
pub enum PeerThreadStatus {
    Started,
    Connected,
    Disconnected,
    Finished,
}

#[derive(Debug)]
pub struct PeerThread {
    pub thread: Option<thread::JoinHandle<()>>,
    pub peer: Option<Arc<Peer>>,
    pub status: PeerThreadStatus,
    pub running: Arc<AtomicBool>,
    pub started_at: Instant,
}
