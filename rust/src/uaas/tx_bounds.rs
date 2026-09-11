//! Bounds checking for raw transaction bytes.
//!
//! `Tx::read` in `chain-gang` sizes four allocations directly from
//! attacker-controlled varints, before reading the bytes those varints claim
//! to describe:
//!
//! ```text
//! Tx::read      Vec::with_capacity(n_inputs)      // input count
//! Tx::read      Vec::with_capacity(n_outputs)     // output count
//! TxIn::read    vec![0; script_len]               // unlocking script length
//! TxOut::read   vec![0; script_len]               // locking script length
//! ```
//!
//! A 27-byte transaction declaring a 2^48-byte locking script produces
//! `memory allocation of 281474976710656 bytes failed`. Allocation failure in
//! Rust is an **abort, not a panic**: it does not unwind, so `panic::set_hook`
//! and every `catch_unwind` wrapper are bypassed and the process dies.
//!
//! This module walks the same structure without allocating anything, checking
//! every declared length against the bytes actually present, so a hostile
//! transaction can be rejected before a deserialiser is asked to trust it.
//!
//! # Coverage
//!
//! This guards the HTTP path only: `POST /tx/raw`, which is unauthenticated
//! whenever `api_key` is unset (the documented default) and which
//! `docker-compose.yml` binds to `0.0.0.0:8081`.
//!
//! **The P2P path is not covered and cannot be covered here.** `chain-gang`
//! parses the wire message before this crate sees it — `EventHandler::on_tx`
//! receives an already-constructed `&Tx` — so the allocation happens inside
//! the library, above any code this crate controls. Any connected peer can
//! still abort the process by announcing such a transaction, because
//! `Logic::on_inv` issues `GetData` for every transaction announced to it.
//! Closing that path needs the bound in `chain-gang` itself: CS-400.
//!
//! The walk is bounded by the length of its input. Each input consumes at least
//! [`MIN_INPUT_BYTES`] and each output at least [`MIN_OUTPUT_BYTES`], and a
//! declared count is rejected up front when it could not fit in the bytes
//! remaining, so a declared count of 2^64 costs one multiplication rather than
//! 2^64 iterations.

use std::fmt;

/// Outpoint (32-byte hash + 4-byte index) + shortest possible script length
/// varint + 4-byte sequence.
const MIN_INPUT_BYTES: u64 = 32 + 4 + 1 + 4;

/// 8-byte satoshi amount + shortest possible script length varint.
const MIN_OUTPUT_BYTES: u64 = 8 + 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxBoundsError {
    /// The buffer ended in the middle of a fixed-size field.
    Truncated {
        field: &'static str,
        needed: usize,
        remaining: usize,
    },
    /// A varint declared a length or count larger than the bytes available.
    /// This is the shape that causes the abort.
    DeclaredTooLarge {
        field: &'static str,
        declared: u64,
        remaining: usize,
    },
    /// Bytes left over after a complete transaction. The parser would ignore
    /// them and return a txid that does not correspond to the submitted bytes.
    TrailingBytes { count: usize },
}

impl fmt::Display for TxBoundsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TxBoundsError::Truncated {
                field,
                needed,
                remaining,
            } => write!(
                f,
                "transaction truncated reading {field}: needed {needed} bytes, {remaining} remaining"
            ),
            TxBoundsError::DeclaredTooLarge {
                field,
                declared,
                remaining,
            } => write!(
                f,
                "transaction declares {field} of {declared} bytes but only {remaining} remain"
            ),
            TxBoundsError::TrailingBytes { count } => {
                write!(f, "{count} trailing bytes after end of transaction")
            }
        }
    }
}

impl std::error::Error for TxBoundsError {}

/// A non-allocating cursor over the raw bytes.
struct Walker<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Walker<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Walker { bytes, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    /// Advance over `needed` bytes, or fail if they are not there.
    fn skip(&mut self, needed: usize, field: &'static str) -> Result<(), TxBoundsError> {
        if self.remaining() < needed {
            return Err(TxBoundsError::Truncated {
                field,
                needed,
                remaining: self.remaining(),
            });
        }
        self.pos += needed;
        Ok(())
    }

    /// Advance over a length that was declared by the transaction itself.
    /// Separate from `skip` so the error names the attack rather than a
    /// truncation, and so the `u64 -> usize` narrowing is checked in one place.
    fn skip_declared(&mut self, declared: u64, field: &'static str) -> Result<(), TxBoundsError> {
        let remaining = self.remaining();
        // usize is 64-bit on every target this service builds for, but compare
        // as u64 so a 32-bit target cannot truncate a hostile length into a
        // small one.
        if declared > remaining as u64 {
            return Err(TxBoundsError::DeclaredTooLarge {
                field,
                declared,
                remaining,
            });
        }
        self.pos += declared as usize;
        Ok(())
    }

    /// Read a Bitcoin varint: < 0xfd is the value itself, 0xfd/0xfe/0xff
    /// introduce a 2/4/8-byte little-endian value.
    fn read_varint(&mut self, field: &'static str) -> Result<u64, TxBoundsError> {
        let first = *self.bytes.get(self.pos).ok_or(TxBoundsError::Truncated {
            field,
            needed: 1,
            remaining: 0,
        })?;
        self.pos += 1;

        let width = match first {
            0xff => 8,
            0xfe => 4,
            0xfd => 2,
            value => return Ok(u64::from(value)),
        };

        if self.remaining() < width {
            return Err(TxBoundsError::Truncated {
                field,
                needed: width,
                remaining: self.remaining(),
            });
        }

        let mut buf = [0u8; 8];
        buf[..width].copy_from_slice(&self.bytes[self.pos..self.pos + width]);
        self.pos += width;
        Ok(u64::from_le_bytes(buf))
    }

    /// Reject a declared count that could not possibly fit, before looping over
    /// it. This is what keeps the walk bounded: without it, a count of 2^64
    /// would be rejected only on the first iteration's `skip`, which is still
    /// correct but leaves the bound implicit.
    fn ensure_count_fits(
        &self,
        count: u64,
        min_each: u64,
        field: &'static str,
    ) -> Result<(), TxBoundsError> {
        let remaining = self.remaining();
        let fits = count
            .checked_mul(min_each)
            .is_some_and(|needed| needed <= remaining as u64);
        if fits {
            Ok(())
        } else {
            Err(TxBoundsError::DeclaredTooLarge {
                field,
                declared: count,
                remaining,
            })
        }
    }
}

/// Walk `bytes` as a serialised transaction, checking every declared length
/// against the bytes actually present. Allocates nothing and never reads
/// outside the slice.
///
/// Returning `Ok` means the structure is self-consistent and safe to hand to
/// `Tx::read`. It says nothing about whether the transaction is valid: scripts
/// are not parsed, signatures are not checked, amounts are not summed.
pub fn validate_tx_bytes(bytes: &[u8]) -> Result<(), TxBoundsError> {
    let mut walker = Walker::new(bytes);

    walker.skip(4, "version")?;

    let n_inputs = walker.read_varint("input count")?;
    walker.ensure_count_fits(n_inputs, MIN_INPUT_BYTES, "input count")?;
    for _ in 0..n_inputs {
        walker.skip(36, "outpoint")?;
        let script_len = walker.read_varint("unlocking script length")?;
        walker.skip_declared(script_len, "unlocking script")?;
        walker.skip(4, "sequence")?;
    }

    let n_outputs = walker.read_varint("output count")?;
    walker.ensure_count_fits(n_outputs, MIN_OUTPUT_BYTES, "output count")?;
    for _ in 0..n_outputs {
        walker.skip(8, "satoshis")?;
        let script_len = walker.read_varint("locking script length")?;
        walker.skip_declared(script_len, "locking script")?;
    }

    walker.skip(4, "lock time")?;

    let left = walker.remaining();
    if left != 0 {
        return Err(TxBoundsError::TrailingBytes { count: left });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chain_gang::{messages::Tx, util::Serializable};
    use std::io::Cursor;

    fn varint(n: u64) -> Vec<u8> {
        if n < 0xfd {
            vec![n as u8]
        } else if n <= u64::from(u16::MAX) {
            let mut v = vec![0xfd];
            v.extend_from_slice(&(n as u16).to_le_bytes());
            v
        } else if n <= u64::from(u32::MAX) {
            let mut v = vec![0xfe];
            v.extend_from_slice(&(n as u32).to_le_bytes());
            v
        } else {
            let mut v = vec![0xff];
            v.extend_from_slice(&n.to_le_bytes());
            v
        }
    }

    /// One output whose script length varint says `declared` but which is
    /// followed by `supplied` actual bytes.
    fn tx_with_output_script(declared: u64, supplied: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u32.to_le_bytes()); // version
        bytes.push(0x00); // no inputs
        bytes.push(0x01); // one output
        bytes.extend_from_slice(&0i64.to_le_bytes()); // satoshis
        bytes.extend_from_slice(&varint(declared));
        bytes.extend_from_slice(supplied);
        bytes.extend_from_slice(&0u32.to_le_bytes()); // lock_time
        bytes
    }

    fn tx_with_input_script(declared: u64, supplied: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u32.to_le_bytes()); // version
        bytes.push(0x01); // one input
        bytes.extend_from_slice(&[0u8; 32]); // prev hash
        bytes.extend_from_slice(&0u32.to_le_bytes()); // prev index
        bytes.extend_from_slice(&varint(declared));
        bytes.extend_from_slice(supplied);
        bytes.extend_from_slice(&0xffff_ffffu32.to_le_bytes()); // sequence
        bytes.push(0x00); // no outputs
        bytes.extend_from_slice(&0u32.to_le_bytes()); // lock_time
        bytes
    }

    // ----- the four attacker-controlled allocation sites -----
    //
    // None of these call Tx::read. That is the point: on these inputs the
    // deserialiser aborts the process, so there is nothing for a test to
    // observe. The validator has to reject them first.

    // The reproduction: 27 bytes declaring a 2^48-byte locking script. Against
    // Tx::read this prints "memory allocation of 281474976710656 bytes failed"
    // and raises SIGABRT.
    #[test]
    fn bounds01_hostile_locking_script_length_is_rejected() {
        let bytes = tx_with_output_script(1 << 48, &[]);
        assert_eq!(bytes.len(), 27, "the reproduction is 27 bytes");
        assert_eq!(
            validate_tx_bytes(&bytes),
            Err(TxBoundsError::DeclaredTooLarge {
                field: "locking script",
                declared: 1 << 48,
                remaining: 4,
            })
        );
    }

    #[test]
    fn bounds02_hostile_unlocking_script_length_is_rejected() {
        let bytes = tx_with_input_script(u64::MAX, &[]);
        assert!(matches!(
            validate_tx_bytes(&bytes),
            Err(TxBoundsError::DeclaredTooLarge {
                field: "unlocking script",
                ..
            })
        ));
    }

    #[test]
    fn bounds03_hostile_input_count_is_rejected() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&varint(u64::MAX)); // Vec::with_capacity(2^64-1)
        bytes.push(0x00);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            validate_tx_bytes(&bytes),
            Err(TxBoundsError::DeclaredTooLarge {
                field: "input count",
                declared: u64::MAX,
                ..
            })
        ));
    }

    #[test]
    fn bounds04_hostile_output_count_is_rejected() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(0x00); // no inputs
        bytes.extend_from_slice(&varint(u64::MAX)); // Vec::with_capacity(2^64-1)
        bytes.extend_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            validate_tx_bytes(&bytes),
            Err(TxBoundsError::DeclaredTooLarge {
                field: "output count",
                declared: u64::MAX,
                ..
            })
        ));
    }

    // A count that overflows u64 when multiplied by the per-item minimum must
    // be rejected rather than wrapping to something small.
    #[test]
    fn bounds05_count_that_overflows_the_capacity_check_is_rejected() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        // u64::MAX / 41 still overflows when multiplied by MIN_INPUT_BYTES.
        bytes.extend_from_slice(&varint(u64::MAX / 2));
        bytes.push(0x00);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            validate_tx_bytes(&bytes),
            Err(TxBoundsError::DeclaredTooLarge { .. })
        ));
    }

    // ----- the legitimate path must not regress -----

    #[test]
    fn bounds06_well_formed_transactions_are_accepted_and_parse() {
        let p2pkh = {
            let mut s = vec![0x76, 0xa9, 0x14];
            s.extend_from_slice(&[0x11; 20]);
            s.extend_from_slice(&[0x88, 0xac]);
            s
        };

        for (label, bytes) in [
            ("no inputs or outputs", tx_with_output_script(0, &[])),
            ("one p2pkh output", tx_with_output_script(25, &p2pkh)),
            (
                "one input with a script",
                tx_with_input_script(3, &[0x51, 0x52, 0x53]),
            ),
            ("empty unlocking script", tx_with_input_script(0, &[])),
        ] {
            assert_eq!(
                validate_tx_bytes(&bytes),
                Ok(()),
                "{label} should be accepted"
            );
            assert!(
                Tx::read(&mut Cursor::new(&bytes)).is_ok(),
                "{label} should also parse, or the validator disagrees with the deserialiser"
            );
        }
    }

    // The size cap is the wrong control for this bug, but it must still work.
    #[test]
    fn bounds07_transaction_at_the_broadcast_size_limit_is_accepted() {
        const LIMIT: usize = 1_000_000;
        // 4 version + 1 input count + 1 output count + 8 satoshis
        // + 5 script length varint + 4 lock time = 23 bytes of frame.
        let script_len = LIMIT - 23;
        let script = vec![0x51u8; script_len];
        let bytes = tx_with_output_script(script_len as u64, &script);

        assert_eq!(bytes.len(), LIMIT);
        assert_eq!(validate_tx_bytes(&bytes), Ok(()));
        assert!(Tx::read(&mut Cursor::new(&bytes)).is_ok());
    }

    // ----- truncation -----

    #[test]
    fn bounds08_truncated_fields_are_rejected() {
        let full = tx_with_output_script(25, &[0x51; 25]);
        for cut in 1..full.len() {
            let truncated = &full[..cut];
            assert!(
                validate_tx_bytes(truncated).is_err(),
                "a {cut}-byte prefix of a {}-byte transaction should be rejected",
                full.len()
            );
        }
    }

    // Safe to hand to the deserialiser: the declared length is small, so the
    // allocation succeeds and the read fails honestly. Both must reject.
    #[test]
    fn bounds09_short_declared_length_is_rejected_by_both() {
        let bytes = tx_with_output_script(25, &[0x51; 2]);
        assert!(matches!(
            validate_tx_bytes(&bytes),
            Err(TxBoundsError::DeclaredTooLarge {
                field: "locking script",
                declared: 25,
                ..
            })
        ));
        assert!(Tx::read(&mut Cursor::new(&bytes)).is_err());
    }

    #[test]
    fn bounds10_empty_input_is_rejected() {
        assert!(matches!(
            validate_tx_bytes(&[]),
            Err(TxBoundsError::Truncated {
                field: "version",
                ..
            })
        ));
    }

    // A padded submission parses as a valid transaction and returns a txid that
    // does not correspond to the bytes that were sent.
    #[test]
    fn bounds11_trailing_bytes_are_rejected() {
        let mut bytes = tx_with_output_script(0, &[]);
        bytes.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(
            validate_tx_bytes(&bytes),
            Err(TxBoundsError::TrailingBytes { count: 4 })
        );
        // The deserialiser is happy with it, which is the problem.
        assert!(Tx::read(&mut Cursor::new(&bytes)).is_ok());
    }
}
