//! Fuzz the pattern compiler: `ScriptMatcher::compile`.
//!
//! This is the surface `POST /collection/monitor` exposes. The handler takes a
//! caller-supplied `locking_script_pattern` and compiles it, so every byte of
//! the input here is attacker-chosen in production.
//!
//! The property is not "compiles" — most inputs are expected to be rejected,
//! and rejecting is the correct answer. The property is that **compiling never
//! panics, never aborts and never allocates without bound**, whatever it is
//! handed. A panic here would take down the thread that compiles monitors; an
//! abort would take down the process, which is the CS-395 failure mode.
//!
//! A compiled matcher is then used, because a pattern that compiles into
//! something that panics on its first match is the same bug one step later.

#![no_main]

use libfuzzer_sys::fuzz_target;
use uaas::uaas::hex_pattern::ScriptMatcher;

fuzz_target!(|pattern: &str| {
    let Ok(matcher) = ScriptMatcher::compile(pattern) else {
        // Rejection is the expected outcome for almost everything. The
        // notation is deliberately narrow: anything it cannot translate
        // faithfully is a configuration error, not something to approximate.
        return;
    };

    // Whatever compiled must survive being used. These three inputs are the
    // shapes that have historically broken matchers: nothing, a real script,
    // and a long run of a single byte.
    let _ = matcher.is_match(&[]);
    let _ = matcher.is_match(&hex_p2pkh());
    let _ = matcher.is_match(&[0x00; 4096]);
});

/// A genuine 25-byte P2PKH locking script.
fn hex_p2pkh() -> Vec<u8> {
    let mut script = Vec::with_capacity(25);
    script.extend_from_slice(&[0x76, 0xa9, 0x14]);
    script.extend_from_slice(&[0xc0; 20]);
    script.extend_from_slice(&[0x88, 0xac]);
    script
}
