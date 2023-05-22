use chain_gang::peer::Peer;
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use crate::peer_thread::{PeerThread, PeerThreadStatus};

pub struct ThreadTracker {
    // Used to track peer connection threads
    children: HashMap<IpAddr, PeerThread>,
}

impl ThreadTracker {
    pub fn new() -> Self {
        ThreadTracker {
            children: HashMap::new(),
        }
    }

    pub fn add(&mut self, ip: IpAddr, peer: PeerThread) {
        self.children.insert(ip, peer);
    }

    pub fn print(&self) {
        for (ip, child) in &self.children {
            println!(
                "ip={}, result={:?}, time={}s",
                ip,
                child,
                child.started_at.elapsed().as_secs()
            );
        }
    }

    pub fn all_finished(&self) -> bool {
        // Return true if all threads have finished
        self.children
            .iter()
            .all(|(_, child)| child.status == PeerThreadStatus::Finished)
    }

    pub fn set_status(&mut self, ip: &IpAddr, status: PeerThreadStatus) {
        // note this quietly fails if not found
        if let Some(x) = self.children.get_mut(ip) {
            x.status = status;
        }
    }

    pub fn get_connected_peer(&self) -> Option<&Arc<Peer>> {
        // Return a channel to a connected peer EventHandler
        for child in self.children.values() {
            if child.status == PeerThreadStatus::Connected {
                // return Some(&child.request_tx);
                return child.peer.as_ref();
            }
        }
        None
    }

    pub fn stop_all(&mut self) {
        // stop all threads
        let _ = self
            .children
            .iter_mut()
            .map(|(_ip, thx)| thx.running.store(false, Ordering::Relaxed))
            .collect::<Vec<_>>();
    }

    pub fn stop(&mut self, ip: &IpAddr) {
        // Stop the thread from waiting for messages
        // (Speeds up shutdown)
        if let Some(x) = self.children.get_mut(ip) {
            x.running.store(false, Ordering::Relaxed);
        }
    }

    pub fn join_thread(&mut self, ip: &IpAddr) {
        // Joins the thread (wait for it to finish)
        // remove required to move thread out of HashMap
        if let Some(peer) = self.children.remove(ip) {
            // Determine when thread started
            let started_at = peer.started_at;

            if let Some(thread) = peer.thread {
                // wait for it
                thread.join().unwrap();

                // Create a new entry to replace the existing one
                let new_peer = PeerThread {
                    thread: None,
                    status: PeerThreadStatus::Finished,
                    running: Arc::new(AtomicBool::new(false)),
                    started_at,
                    peer: None,
                };
                self.children.insert(*ip, new_peer);
            }
        }
    }
}
