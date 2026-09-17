//! Fuzz the matcher against arbitrary locking script bytes.
//!
//! The pattern is fixed and the *script* is the fuzzed input, which is the way
//! round that matters: a locking script arrives from the P2P network inside a
//! transaction, so its bytes are chosen by whoever built the transaction. Post
//! Genesis there is no script size cap, no 520-byte element cap and no stack
//! element count cap, so the only real ceiling is the 32 MiB P2P payload.
//!
//! Two properties.
//!
//! **Never panic, never abort.** The matcher runs on the same thread as block
//! processing, so a panic here stalls the indexer and an abort ends it.
//!
//! **A match is byte-aligned.** The `identifier` group below is written
//! `[0-9a-f]{40}`, which the notation defines as 40 hex characters — that is,
//! exactly 20 bytes. If a capture ever comes back a different length, the
//! translation from hex notation to bytes has broken, and a matcher that can
//! capture a partial byte is the nibble-misalignment bug of CS-402 returning.
//! This is the assertion that makes the target worth running rather than just
//! a crash detector.

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::sync::LazyLock;
use uaas::uaas::hex_pattern::ScriptMatcher;

/// The live `Fin` collection's shape, with the pubkeyhash captured. A fixed
/// pattern keeps the input space to the script alone.
const PATTERN: &str = "76a914(?<identifier>[0-9a-f]{40})88ac";

/// 40 hex characters is 20 bytes. This is the invariant under test.
const IDENTIFIER_BYTES: usize = 20;

static MATCHER: LazyLock<ScriptMatcher> =
    LazyLock::new(|| ScriptMatcher::compile(PATTERN).expect("the fixed pattern compiles"));

fuzz_target!(|script: &[u8]| {
    if !MATCHER.is_match(script) {
        return;
    }

    // It matched, so the declared identifier must have participated and must
    // describe whole bytes.
    let captured = MATCHER
        .identifier(script)
        .expect("a matching script must yield the declared identifier");

    assert_eq!(
        captured.len(),
        IDENTIFIER_BYTES,
        "identifier must be {IDENTIFIER_BYTES} whole bytes, got {} from script {:02x?}",
        captured.len(),
        script
    );
});
