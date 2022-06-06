use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use std::sync::Arc;

use crate::event_handler::RequestMessage;
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

    pub fn get_request_tx(&self, ip: &IpAddr) -> Option<&mpsc::Sender<RequestMessage>> {
        if let Some(x) = self.children.get(ip) {
            Some(&x.request_tx)
        } else {
            None
        }
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
            let request_tx = peer.request_tx;

            if let Some(thread) = peer.thread {
                // wait for it
                thread.join().unwrap();

                // Create a new entry to replace the existing one
                let new_peer = PeerThread {
                    thread: None,
                    status: PeerThreadStatus::Finished,
                    running: Arc::new(AtomicBool::new(false)),
                    started_at,
                    request_tx,
                };
                self.children.insert(*ip, new_peer);
            }
        }
    }
}
