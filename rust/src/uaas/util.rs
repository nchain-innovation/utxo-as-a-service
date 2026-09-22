use chain_gang::messages::Tx;

/// The sum of a transaction's output amounts, or `None` if it does not fit.
///
/// `Iterator::sum` on `i64` panics in a debug build and **wraps silently in a
/// release one**, and `release_max_level_warn` compiles out anything below
/// `warn`, so the release binary — the one that is published — is exactly
/// where a wrapped total would go unnoticed. Output amounts arrive straight
/// off the P2P network and nothing validates them against the supply cap, so
/// they are untrusted input (CS-435).
///
/// **Checked rather than saturating.** A saturated total is a plausible number
/// that is wrong, and every caller here already has a "cannot determine this"
/// path to fall back on, so there is nothing to gain by inventing a value.
///
/// Note what this does *not* do: a negative `satoshis` is out of range for a
/// real output but is representable in `i64` and arrives unvalidated, and this
/// sums it as given. Rejecting it is a validity question rather than an
/// arithmetic one, and belongs with whatever else decides a transaction is
/// well formed.
pub fn sum_output_satoshis(tx: &Tx) -> Option<i64> {
    tx.outputs
        .iter()
        .try_fold(0i64, |total, vout| total.checked_add(vout.satoshis))
}

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

    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(err) => {
            log::warn!("Unable to read system time: {err:?}");
            return 0;
        }
    };

    now.saturating_sub(block_timestamp)
}

// Decode hex
// from https://play.rust-lang.org/?version=stable&mode=debug&edition=2015&gist=e241493d100ecaadac3c99f37d0f766f

pub fn decode_hexstr(s: &str) -> Result<Vec<u8>, DecodeHexError> {
    if !s.len().is_multiple_of(2) {
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

#[cfg(test)]
mod tests {
    use super::sum_output_satoshis;
    use chain_gang::messages::{Tx, TxOut};
    use chain_gang::script::Script;

    fn tx_paying(amounts: &[i64]) -> Tx {
        Tx {
            version: 1,
            inputs: Vec::new(),
            outputs: amounts
                .iter()
                .map(|satoshis| TxOut {
                    satoshis: *satoshis,
                    lock_script: Script(vec![0x51]),
                })
                .collect(),
            lock_time: 0,
        }
    }

    #[test]
    fn sat01_an_ordinary_total_is_returned() {
        assert_eq!(
            sum_output_satoshis(&tx_paying(&[1_000, 2_500, 7])),
            Some(3_507)
        );
    }

    /// The case the whole change exists for. Unchecked this panicked in a
    /// debug build and wrapped in a release one, and `release_max_level_warn`
    /// meant the wrapped value was never logged — so the published binary was
    /// the one that would have carried a wrong number silently.
    #[test]
    fn sat02_a_total_that_does_not_fit_is_none_rather_than_a_panic_or_a_wrap() {
        assert_eq!(sum_output_satoshis(&tx_paying(&[i64::MAX, 1])), None);
        assert_eq!(sum_output_satoshis(&tx_paying(&[i64::MAX, i64::MAX])), None);
        // Overflow found part way through still stops, rather than continuing
        // with a wrapped accumulator.
        assert_eq!(sum_output_satoshis(&tx_paying(&[i64::MAX, 1, -5])), None);
    }

    #[test]
    fn sat03_no_outputs_is_zero_not_an_error() {
        assert_eq!(sum_output_satoshis(&tx_paying(&[])), Some(0));
    }

    /// A negative amount is out of range for a real output but is
    /// representable and arrives unvalidated. Summed as given, deliberately:
    /// rejecting it is a validity question, not an arithmetic one. This pins
    /// the stated limit so it cannot quietly change.
    #[test]
    fn sat04_a_negative_amount_is_summed_as_given() {
        assert_eq!(sum_output_satoshis(&tx_paying(&[1_000, -400])), Some(600));
        assert_eq!(sum_output_satoshis(&tx_paying(&[i64::MIN, -1])), None);
    }
}
