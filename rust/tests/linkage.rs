//! A standing guard that this crate keeps a library target.
//!
//! This file exists to fail the build if `src/lib.rs` is ever removed, or the
//! modules an integration test needs stop being public. A binary-only crate
//! exposes nothing for an integration test to link, so before CS-414 this file
//! could not compile at all:
//!
//! ```text
//! error[E0433]: cannot find module or crate `uaas`
//!   |             ^^^^ use of unresolved module or unlinked crate `uaas`
//! ```
//!
//! That is the same wall `cargo fuzz` hits, since a fuzz target is a separate
//! crate depending on this one as a library (CS-413). The assertion below is
//! deliberately trivial — the linking is the test.
//!
//! Note what is *not* reachable from here: a `#[cfg(test)]` module such as
//! `uaas::script_asm` is compiled only for the library's own test build, not
//! for this separate crate. Anything an integration test or fuzz target must
//! call has to be an ordinary module, or gated on a cargo feature rather than
//! on `cfg(test)`.

use uaas::uaas::tx_bounds::validate_tx_bytes;

#[test]
fn link01_an_integration_test_can_call_into_the_library() {
    // Empty input is not a transaction. The point is that this resolves at all.
    assert!(validate_tx_bytes(&[]).is_err());
}
