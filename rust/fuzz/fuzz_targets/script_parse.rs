//! Fuzz the script tokeniser against arbitrary bytes.
//!
//! Every byte of a locking script is chosen by whoever built the transaction,
//! and post-Genesis nothing caps its size, its elements or its nesting. So the
//! input here is exactly the input in production: arbitrary.
//!
//! Four properties, each of which would be a real defect if violated.
//!
//! **It terminates.** Every token must consume at least one byte, so the walk
//! cannot loop forever on a crafted script. Asserted directly against the
//! offsets rather than trusted.
//!
//! **It never panics and never aborts.** The tokeniser runs on the same thread
//! as block processing. A panic stalls the indexer; an allocation failure or a
//! stack overflow ends the process, because neither unwinds.
//!
//! **It stays inside the script.** Every offset and every borrowed element must
//! lie within the input. A push declaring four gigabytes must be reported, not
//! read.
//!
//! **It is lossless.** Re-serialising the token stream from `(encoding,
//! element)` alone must reproduce the input byte for byte, up to the point any
//! truncation was reported. This is the property the matcher in CS-415 will
//! depend on: a token stream that silently collapsed a non-minimal push would
//! let an attacker choose an encoding the matcher never sees.

#![no_main]

use libfuzzer_sys::fuzz_target;
use uaas::uaas::script_parse::{tokenise, Token, TokenKind};

fuzz_target!(|script: &[u8]| {
    let mut out = Vec::new();
    let mut previous_offset: Option<usize> = None;
    let mut truncated = false;

    for token in tokenise(script) {
        let Token { kind, offset, .. } = token;

        assert!(
            offset <= script.len(),
            "offset {offset} is outside a {}-byte script",
            script.len()
        );

        // Progress: offsets are strictly increasing, so the walk terminates.
        if let Some(previous) = previous_offset {
            assert!(
                offset > previous,
                "offset did not advance: {previous} then {offset}"
            );
        }
        previous_offset = Some(offset);

        assert!(!truncated, "a truncation marker must be the final token");

        match kind {
            TokenKind::Push { element, .. } | TokenKind::TrailingData(element) => {
                assert!(
                    element.len() <= script.len(),
                    "a {}-byte element cannot come from a {}-byte script",
                    element.len(),
                    script.len()
                );
            }
            TokenKind::Truncated(_) => truncated = true,
            _ => {}
        }

        token.write_to(&mut out);
    }

    if truncated {
        // The bytes before the truncation point must still round-trip; the
        // marker itself describes bytes that were not there.
        assert!(
            script.starts_with(&out),
            "the tokens before a truncation must be a prefix of the input"
        );
    } else {
        assert_eq!(
            out, script,
            "a fully parsed script must round-trip exactly"
        );
    }
});
