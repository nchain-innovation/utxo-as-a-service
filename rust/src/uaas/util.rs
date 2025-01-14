use chrono::*;
//{format::ParseError, prelude::DateTime, Utc};

use std::{
    fmt,
    num::ParseIntError,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub fn timestamp_as_string(timestamp: u32) -> String {
    // Convert block timestamp to something readable
    let seconds: u64 = timestamp.into();
    let d = UNIX_EPOCH + Duration::from_secs(seconds);
    let datetime = DateTime::<Utc>::from(d);
    let timestamp_str = datetime.format("%Y-%m-%d %H:%M:%S").to_string();
    timestamp_str
}

pub fn delay_as_string(secs: u64) -> String {
    let seconds = secs % 60;
    let mins = (secs / 60) % 60;
    let hours = secs / 3600; //% 24;
                             //let day = (sec / (3600 * 24));
    format!("{}:{:0>2}:{:0>2}", hours, mins, seconds)
}

pub fn timestamp_age_as_sec(timestamp: u32) -> u64 {
    // Return the age of the block timestamp (against current time) in seconds
    let block_timestamp: u64 = timestamp.into();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    now.saturating_sub(block_timestamp)
}

// Decode hex
// from https://play.rust-lang.org/?version=stable&mode=debug&edition=2015&gist=e241493d100ecaadac3c99f37d0f766f

pub fn decode_hexstr(s: &str) -> Result<Vec<u8>, DecodeHexError> {
    if s.len() % 2 != 0 {
        Err(DecodeHexError::OddLength)
    } else {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.into()))
            .collect()
    }
}
/*
const HEX_BYTES: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f\
                         202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f\
                         404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f\
                         606162636465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f\
                         808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f\
                         a0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebf\
                         c0c1c2c3c4c5c6c7c8c9cacbcccdcecfd0d1d2d3d4d5d6d7d8d9dadbdcdddedf\
                         e0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f2f3f4f5f6f7f8f9fafbfcfdfeff";

pub fn encode_hexstr(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| unsafe {
            let i = 2 * b as usize;
            HEX_BYTES.get_unchecked(i..i + 2)
        })
        .collect()
}
*/

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeHexError {
    OddLength,
    ParseInt(ParseIntError),
}

impl From<ParseIntError> for DecodeHexError {
    fn from(e: ParseIntError) -> Self {
        DecodeHexError::ParseInt(e)
    }
}

impl fmt::Display for DecodeHexError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DecodeHexError::OddLength => "input string has an odd number of bytes".fmt(f),
            DecodeHexError::ParseInt(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for DecodeHexError {}
