//! The adversarial fixture corpus for the collection matcher.
//!
//! Each probe here corresponds to a lettered probe in the review findings
//! (`utxo-service-review-findings.md`). The originals were written in a
//! scratchpad that has since been cleared; these are reconstructions from the
//! findings' specifications, not copies.
//!
//! # What a probe asserts
//!
//! Most of these probes assert behaviour the team intends to **change**. The
//! matcher is a substring search over bytes: it has no notion of opcode
//! boundaries, execution paths, or the script/data split, so it cannot
//! distinguish "this output pays the monitored key" from "these bytes appear
//! somewhere in this output". Probes A, B, C, D and E each demonstrate one
//! consequence of that.
//!
//! A test that asserts something we intend to change is a trap unless it says
//! so, so every such probe is named `..._today` and carries a
//! `TODO(CS-415)` comment. When the structural matcher lands, the assertion
//! inverts and the suffix goes.
//!
//! # Where this lives, and why
//!
//! In `src/` under `#[cfg(test)]`, not in `rust/tests/`.
//!
//! CS-414 gave the crate a library target, so an integration test *can* link
//! it — `tests/linkage.rs` does. But `script_asm` is `#[cfg(test)]`, and an
//! integration test is a separate crate linking the library built **without**
//! `cfg(test)`, so the assembler is not merely private there, it is absent:
//!
//! ```text
//! error[E0432]: unresolved import `uaas::uaas::script_asm`
//! note: found an item that was configured out
//! ```
//!
//! Every probe below needs the assembler. Moving both modules behind a
//! `testing` cargo feature would let them live in `tests/`, but nothing needs
//! that yet; the fuzz target in CS-413 is the first thing that will, and it
//! can promote both in one move.
//!
//! # Fixtures
//!
//! Synthetic only. No probe connects to or replays from a mainnet node.

use chain_gang::{
    address::{addr_encode, AddressType},
    messages::{Tx, TxOut},
    network::Network,
    script::Script,
    util::Hash160,
};

use crate::{
    config::CollectionConfig,
    uaas::{collection::WorkingCollection, script_asm::assemble},
};

/// The pubkeyhash monitored by the live `Fin` collection in `data/uaasr.toml`.
/// Used verbatim so the probes describe the deployed configuration rather than
/// a sanitised version of it.
const FIN_H160: &str = "c0d164cbb336e3c64338c70506ef543c2fc7b8f9";

/// The pattern `Fin` is configured with, generalised over the hash. This is the
/// shape every address-derived pattern also takes.
const P2PKH_PATTERN: &str = "76a914[0-9a-f]{40}88ac";

/// A syntactically well-formed compressed public key. Nothing in the matcher
/// validates a point on the curve, which is part of the finding.
const PUBKEY: &str = "02111111111111111111111111111111111111111111111111111111111111111e";

/// The pattern a "monitor this key" collection would be configured with: the
/// 33-byte push opcode followed by the key.
fn pubkey_pattern() -> String {
    format!("21{PUBKEY}")
}

/// A genuine 25-byte P2PKH locking script as hex, for fixtures that shift it.
fn genuine_p2pkh_hex() -> String {
    format!("76a914{}88ac", "aa".repeat(20))
}

fn asm(src: &str) -> Vec<u8> {
    assemble(src).unwrap_or_else(|err| panic!("fixture must assemble: {src:?}: {err:?}"))
}

fn collection_for(pattern: &str) -> WorkingCollection {
    WorkingCollection::new(
        CollectionConfig {
            name: "probe".to_string(),
            track_descendants: false,
            address: None,
            locking_script_pattern: Some(pattern.to_string()),
        },
        Network::BSV_Testnet,
    )
    .expect("probe pattern compiles")
}

fn collection_for_address(address: &str) -> WorkingCollection {
    WorkingCollection::new(
        CollectionConfig {
            name: "probe-address".to_string(),
            track_descendants: false,
            address: Some(address.to_string()),
            locking_script_pattern: None,
        },
        Network::BSV_Testnet,
    )
    .expect("probe address compiles")
}

/// A transaction with one output carrying `script`. Only the locking script is
/// under test; the value and the input set are irrelevant to the matcher.
fn tx_with_script(script: Vec<u8>) -> Tx {
    Tx {
        version: 1,
        inputs: Vec::new(),
        outputs: vec![TxOut {
            satoshis: 1_000,
            lock_script: Script(script),
        }],
        lock_time: 0,
    }
}

fn collects(collection: &WorkingCollection, script: Vec<u8>) -> bool {
    collection.match_any_locking_script(&tx_with_script(script))
}

/// The genuine article, so each probe below is a comparison rather than an
/// isolated assertion: the matcher does select a real P2PKH output.
#[test]
fn probe_baseline_a_genuine_p2pkh_output_is_collected() {
    let collection = collection_for(P2PKH_PATTERN);
    let script = asm(&format!(
        "OP_DUP OP_HASH160 0x{FIN_H160} OP_EQUALVERIFY OP_CHECKSIG"
    ));
    assert_eq!(script.len(), 25, "p2pkh is 25 bytes");
    assert!(
        collects(&collection, script),
        "the pattern must still select the output it was written for"
    );
}

/// Probe A — the configured template embedded verbatim in `OP_RETURN` trailing
/// data. The bytes are never executed and the output is unspendable, but the
/// substring is present, so the collection takes it.
///
/// Cost to mount: one dust output.
///
/// TODO(CS-415): a structural matcher must not collect this. Invert to
/// `assert!(!collects(...))` and drop the `_today` suffix.
#[test]
fn probe_a_p2pkh_template_as_op_return_data_is_collected_today() {
    let collection = collection_for(P2PKH_PATTERN);

    // 6a 19 76a914<h160>88ac
    let script = asm(&format!(
        "OP_RETURN 0x76a914{FIN_H160}88ac  # the template as pushed data"
    ));
    assert_eq!(script[0], 0x6a, "must begin with OP_RETURN");

    assert!(
        collects(&collection, script),
        "documents the false positive: unexecuted OP_RETURN data is collected"
    );
}

/// Probe B — `OP_RETURN` nested inside an `OP_IF` branch, template after it.
///
/// This is the case that defeats "treat everything after the first OP_RETURN
/// as data". Post-Genesis, `OP_RETURN` appears both as a script-block
/// production and as the trailing-data delimiter, so the first one in the byte
/// stream is not reliably the boundary. Any tokeniser has to track branch
/// context to tell them apart.
///
/// TODO(CS-415): must not be collected once branch structure is understood.
#[test]
fn probe_b_op_return_inside_a_branch_is_collected_today() {
    let collection = collection_for(P2PKH_PATTERN);

    // 63 6a 68 19 76a914<h160>88ac
    let script = asm(&format!("OP_IF OP_RETURN OP_ENDIF 0x76a914{FIN_H160}88ac"));
    assert_eq!(
        &script[..3],
        &[0x63, 0x6a, 0x68],
        "OP_IF OP_RETURN OP_ENDIF"
    );

    assert!(
        collects(&collection, script),
        "documents that branch structure is invisible to the matcher"
    );
}

/// Probe C — the decisive one. The monitored key is pushed, immediately
/// `OP_DROP`ped, and the script terminates in `OP_FALSE OP_RETURN`. The output
/// is provably unspendable and the key never reaches a signature check, yet the
/// collection hit is indistinguishable from a genuine one.
///
/// This is the difference between the three properties in review question 1:
/// the matcher establishes only "these bytes are present", never "this output
/// is spendable by the holder of that key".
///
/// TODO(CS-415): the match must sit on an executable path terminating in a
/// signature check over that operand.
#[test]
fn probe_c_key_dropped_before_any_checksig_is_collected_today() {
    let collection = collection_for(&pubkey_pattern());

    // 21<pubkey> 75 00 6a
    let script = asm(&format!("0x{PUBKEY} OP_DROP OP_0 OP_RETURN"));
    assert_eq!(script.last(), Some(&0x6a), "terminates in OP_RETURN");

    assert!(
        collects(&collection, script),
        "documents that reachability of a CHECKSIG is not established"
    );

    // Nothing in the crate looks for a signature check at all. If that ever
    // changes, this assertion is the first thing that should fail.
    assert!(
        !collection.collection.track_descendants,
        "probe fixture must not amplify"
    );
}

/// Probe D — the same 33-byte stack element under all four push encodings.
///
/// The element is identical in every case; only the opcode that introduces it
/// differs. A matcher that understood pushes would answer the same way four
/// times. This one answers `true, true, false, false`, and the second `true`
/// is an accident: `OP_PUSHDATA1`'s length byte is `0x21`, which lands exactly
/// where the pattern expects the minimal push opcode.
///
/// Both failure directions are therefore live: two false negatives, and a
/// coincidental match that would disappear if the element were any other
/// length.
///
/// Post-Genesis, and with `nVersion > 1` under Chronicle dropping
/// minimal-encoding enforcement, the encoding is the sender's free choice.
///
/// TODO(CS-415): all four must match, on the element rather than its
/// encoding.
#[test]
fn probe_d_the_same_element_matches_under_only_two_of_four_encodings_today() {
    let collection = collection_for(&pubkey_pattern());

    let cases: [(&str, &str, bool, &str); 4] = [
        (
            "minimal",
            &format!("0x{PUBKEY}"),
            true,
            "the encoding the pattern was written against",
        ),
        (
            "OP_PUSHDATA1",
            &format!("OP_PUSHDATA1 0x{PUBKEY}"),
            true,
            "matches by accident: the 0x21 length byte stands in for the push opcode",
        ),
        (
            "OP_PUSHDATA2",
            &format!("OP_PUSHDATA2 0x{PUBKEY}"),
            false,
            "false negative: 21 00 splits the pattern",
        ),
        (
            "OP_PUSHDATA4",
            &format!("OP_PUSHDATA4 0x{PUBKEY}"),
            false,
            "false negative: 21 00 00 00 splits the pattern",
        ),
    ];

    for (name, src, expected, why) in cases {
        assert_eq!(collects(&collection, asm(src)), expected, "{name}: {why}");
    }
}

/// Probe D, second half — the element really is the same in all four scripts,
/// so the disagreement above is entirely about encoding and not about content.
///
/// Without this, probe D would be consistent with the fixtures simply being
/// different scripts.
#[test]
fn probe_d_the_four_encodings_carry_an_identical_element() {
    let key = hex::decode(PUBKEY).expect("pubkey hex");

    let encodings = [
        (format!("0x{PUBKEY}"), 1usize),
        (format!("OP_PUSHDATA1 0x{PUBKEY}"), 2),
        (format!("OP_PUSHDATA2 0x{PUBKEY}"), 3),
        (format!("OP_PUSHDATA4 0x{PUBKEY}"), 5),
    ];

    for (src, prefix_len) in encodings {
        let bytes = asm(&src);
        assert_eq!(
            &bytes[prefix_len..],
            key.as_slice(),
            "{src}: the pushed element must be the same 33 bytes"
        );
    }
}

/// Probe E — an address-derived pattern is unanchored too.
///
/// `address_to_lock_script` turns the address into the bare hex of its 25-byte
/// locking script and hands that straight to the compiler. There is no `^`,
/// no `$`, and no structural claim: the collection selects any output whose
/// script *contains* those 25 bytes, including a copy inside `OP_RETURN` data.
///
/// This probe exists separately from A because the address path is the one a
/// user reaches through `POST /collection/monitor` without ever writing a
/// pattern, so the failure is invisible to them.
///
/// TODO(CS-415): an address monitor must select only outputs that actually
/// pay that address.
#[test]
fn probe_e_address_derived_pattern_matches_a_forged_copy_today() {
    let hash160 = Hash160(
        hex::decode(FIN_H160)
            .expect("h160 hex")
            .try_into()
            .expect("20 bytes"),
    );
    let address = addr_encode(&hash160, AddressType::P2PKH, Network::BSV_Testnet);
    let collection = collection_for_address(&address);

    let genuine = asm(&format!(
        "OP_DUP OP_HASH160 0x{FIN_H160} OP_EQUALVERIFY OP_CHECKSIG"
    ));
    assert!(
        collects(&collection, genuine.clone()),
        "the address monitor must select a real payment to that address"
    );

    // The same 25 bytes, re-pushed as OP_RETURN data by a third party.
    let forged = asm(&format!("OP_RETURN 0x{}", hex::encode(&genuine)));
    assert!(
        collects(&collection, forged),
        "documents that an address monitor is satisfied by a forged copy"
    );
}

/// Probe G — pattern compilation under hostile input.
///
/// The original probe asked whether the regex engine could be made to
/// backtrack catastrophically. It could not: `regex` is a finite-automaton
/// engine with no backtracking, and worst-case match time is linear in the
/// haystack. That remains true.
///
/// What has changed since the probe was written is more useful. CS-402 replaced
/// `Regex::new(pattern)` with a translation step that accepts only the hex
/// notation the configuration actually uses. Every pathological pattern the
/// original probe fed to the engine is now **rejected at compile time**, so it
/// never reaches the engine at all. That is a smaller attack surface, not just
/// a fast one — and `POST /collection/monitor` accepts a caller-supplied
/// pattern, so the surface is reachable.
///
/// This probe is a standing guard on that: if the grammar is ever widened to
/// admit general regex syntax, these assertions fail.
#[test]
fn probe_g_pathological_patterns_are_rejected_before_they_reach_the_engine() {
    // Verbatim from the original probe, plus two of the same shape.
    let hostile = [
        "(a+)+b",
        "(0[0-9a-f]*)*76a914",
        "([0-9a-f]{2}){1,10000}88ac",
        "(?<identifier>[0-9a-f]*)*",
        "(76a914|76a915)[0-9a-f]{40}",
    ];

    for pattern in hostile {
        let result = WorkingCollection::new(
            CollectionConfig {
                name: "hostile".to_string(),
                track_descendants: false,
                address: None,
                locking_script_pattern: Some(pattern.to_string()),
            },
            Network::BSV_Testnet,
        );
        assert!(
            result.is_err(),
            "{pattern:?} must be rejected, not compiled"
        );
    }

    // A repetition count large enough to exceed the engine's compiled-size
    // limit is also a compile-time refusal rather than a hang.
    let huge = format!("[0-9a-f]{{{}}}", 200_000_000u64);
    assert!(
        crate::uaas::hex_pattern::ScriptMatcher::compile(&huge).is_err(),
        "an unreasonable repetition count must be refused"
    );

    // The longest thing the Python validator permits is 512 characters. A
    // well-formed pattern of that length is still accepted, so the refusals
    // above are about shape and not merely about size.
    let long_but_legal = "ab".repeat(256);
    assert_eq!(long_but_legal.len(), 512);
    assert!(
        crate::uaas::hex_pattern::ScriptMatcher::compile(&long_but_legal).is_ok(),
        "a long but well-formed literal must still compile"
    );
}

/// Probe K — oversized pushes and deep branch nesting.
///
/// These fixtures exist for the tokeniser in CS-413, which does not exist yet.
/// What can be asserted today is that the matcher returns an answer for each of
/// them rather than hanging or dying, which is worth pinning: the matcher runs
/// on the same thread as block processing.
///
/// The tokeniser's requirement is stronger and cannot be tested here — it must
/// walk these **iteratively**. A recursive descent over the nesting fixture
/// would overflow the stack, and a stack overflow in Rust is an abort, not a
/// catchable panic: the same non-recoverable failure mode as the allocation
/// abort in CS-395.
///
/// Post-Genesis there is no script size cap, no 520-byte element cap and no
/// stack element count cap, so none of these are out of bounds.
#[test]
fn probe_k_oversized_pushes_and_deep_nesting_return_an_answer() {
    let collection = collection_for(P2PKH_PATTERN);

    // 10,000 nested OP_IF ... OP_ENDIF with the template at the centre.
    const DEPTH: usize = 10_000;
    let nested = asm(&format!(
        "{} 0x76a914{FIN_H160}88ac {}",
        "OP_IF ".repeat(DEPTH),
        "OP_ENDIF ".repeat(DEPTH)
    ));
    assert_eq!(nested.len(), DEPTH * 2 + 26);
    assert!(
        collects(&collection, nested),
        "the substring is present at any nesting depth, which is the point"
    );

    // A push declaring 4 GiB with nothing following it. Malformed by
    // construction: `raw:` emits the bytes verbatim, with no length prefix and
    // no data.
    let truncated = asm("raw:4effffffff");
    assert_eq!(truncated, vec![0x4e, 0xff, 0xff, 0xff, 0xff]);
    assert!(
        !collects(&collection, truncated),
        "a declared length must not cause an allocation or a match"
    );

    // A genuinely large element, pushed with PUSHDATA4. 1 MiB rather than the
    // 32 MiB P2P ceiling, so the corpus stays fast; the size sweep proper is
    // probe F in hex_pattern::bench_probe.
    let payload = "5a".repeat(1024 * 1024);
    let large = asm(&format!("OP_PUSHDATA4 0x{payload}"));
    assert_eq!(large.len(), 5 + 1024 * 1024);
    assert!(
        !collects(&collection, large),
        "a large non-matching element must not match"
    );
}

/// Writes the fuzz seed corpus from the probe fixtures above.
///
/// The seeds and the probes are the same bytes by construction, so they cannot
/// drift. Run it after changing a fixture:
///
/// ```text
/// UAAS_WRITE_FUZZ_CORPUS=1 cargo test --lib write_fuzz_seed_corpus
/// ```
///
/// Ordinarily it verifies instead of writing, so a fixture change that was not
/// propagated fails the test rather than silently leaving the fuzzer starting
/// from stale inputs. The seeds are committed; the working corpus a run grows
/// from them is not.
#[test]
fn write_fuzz_seed_corpus() {
    use std::path::Path;

    // Scripts for the matcher_script target. Named for the probe they come
    // from, so a crashing input points at the case that produced it.
    let scripts: Vec<(&str, Vec<u8>)> = vec![
        (
            "baseline_genuine_p2pkh",
            asm(&format!(
                "OP_DUP OP_HASH160 0x{FIN_H160} OP_EQUALVERIFY OP_CHECKSIG"
            )),
        ),
        (
            "probe_a_template_in_op_return",
            asm(&format!("OP_RETURN 0x76a914{FIN_H160}88ac")),
        ),
        (
            "probe_b_op_return_in_branch",
            asm(&format!("OP_IF OP_RETURN OP_ENDIF 0x76a914{FIN_H160}88ac")),
        ),
        (
            "probe_c_key_dropped",
            asm(&format!("0x{PUBKEY} OP_DROP OP_0 OP_RETURN")),
        ),
        ("probe_d_minimal_push", asm(&format!("0x{PUBKEY}"))),
        (
            "probe_d_pushdata1",
            asm(&format!("OP_PUSHDATA1 0x{PUBKEY}")),
        ),
        (
            "probe_d_pushdata2",
            asm(&format!("OP_PUSHDATA2 0x{PUBKEY}")),
        ),
        (
            "probe_d_pushdata4",
            asm(&format!("OP_PUSHDATA4 0x{PUBKEY}")),
        ),
        (
            "probe_h_nibble_misaligned",
            // A genuine p2pkh script shifted by one nibble. The hex encoding
            // still contains "76a914...88ac", but the bytes are not a p2pkh
            // script and contain no OP_DUP OP_HASH160, so the byte matcher
            // must not see it. Seeded so the fuzzer explores around the
            // boundary CS-402 closed.
            asm(&format!("raw:0{}0", genuine_p2pkh_hex())),
        ),
        ("probe_k_truncated_pushdata4", asm("raw:4effffffff")),
        (
            "probe_k_nested_branches",
            asm(&format!(
                "{} 0x76a914{FIN_H160}88ac {}",
                "OP_IF ".repeat(64),
                "OP_ENDIF ".repeat(64)
            )),
        ),
        ("empty", Vec::new()),
    ];

    // Patterns for the matcher_pattern target: every live collection pattern,
    // plus the hostile ones from probe G that must stay rejected.
    let patterns: Vec<(&str, &str)> = vec![
        ("live_johns", "7576a914[0-9a-f]{40}88ac$"),
        ("live_dsa", "006a[0-9a-f]{2}53417631[0-9a-f]*"),
        ("live_cocv1", "006a[0-9a-f]{2}436f437631[0-9a-f]*"),
        (
            "live_fin",
            "76a914c0d164cbb336e3c64338c70506ef543c2fc7b8f988ac",
        ),
        (
            "live_1sat",
            "0063036f726451126170706c69636174696f6e2f6273762d323000[0-9a-f]*",
        ),
        ("identifier_group", "76a914(?<identifier>[0-9a-f]{40})88ac"),
        ("hostile_nested_repetition", "(a+)+b"),
        ("hostile_star_group", "(0[0-9a-f]*)*76a914"),
        ("hostile_counted_group", "([0-9a-f]{2}){1,10000}88ac"),
        ("hostile_alternation", "(76a914|76a915)[0-9a-f]{40}"),
        ("odd_length_literal", "76a91"),
    ];

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/seeds");
    let writing = std::env::var("UAAS_WRITE_FUZZ_CORPUS").is_ok();

    let mut seeds: Vec<(std::path::PathBuf, Vec<u8>)> = Vec::new();
    for (name, bytes) in scripts {
        seeds.push((root.join("matcher_script").join(name), bytes));
    }
    for (name, pattern) in patterns {
        seeds.push((
            root.join("matcher_pattern").join(name),
            pattern.as_bytes().to_vec(),
        ));
    }

    for (path, bytes) in seeds {
        if writing {
            std::fs::create_dir_all(path.parent().expect("has a parent")).expect("create seed dir");
            std::fs::write(&path, &bytes).expect("write seed");
            continue;
        }
        let found = std::fs::read(&path).unwrap_or_else(|err| {
            panic!(
                "missing fuzz seed {}: {err}. Regenerate with \
                 UAAS_WRITE_FUZZ_CORPUS=1 cargo test --lib write_fuzz_seed_corpus",
                path.display()
            )
        });
        assert_eq!(
            found,
            bytes,
            "fuzz seed {} is stale; regenerate with \
             UAAS_WRITE_FUZZ_CORPUS=1 cargo test --lib write_fuzz_seed_corpus",
            path.display()
        );
    }
}
