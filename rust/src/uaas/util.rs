use chrono::prelude::DateTime;
use chrono::Utc;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn timestamp_as_string(timestamp: u32) -> String {
    // Convert block timestamp to something readable
    let seconds: u64 = timestamp.into();
    let d = UNIX_EPOCH + Duration::from_secs(seconds);
    let datetime = DateTime::<Utc>::from(d);
    let timestamp_str = datetime.format("%Y-%m-%d %H:%M:%S").to_string();
    timestamp_str
}

pub fn timestamp_age_as_sec(timestamp: u32) -> u64 {
    // Return the age of the block timestamp (against current time) in seconds
    let block_timestamp: u64 = timestamp.into();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    if now > block_timestamp {
        now - block_timestamp
    } else {
        0
    }
}
