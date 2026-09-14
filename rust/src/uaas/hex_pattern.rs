//! Compile a hex-notation locking script pattern into a byte-level regex.
//!
//! Collection patterns are written against the *hex encoding* of a locking
//! script, e.g. `76a914[0-9a-f]{40}88ac`. Matching them by encoding each script
//! to a `String` and running a text regex over it has two problems.
//!
//! **Cost.** The encoding is redone once per output per collection, and it
//! dominates the match it feeds — at 32 MB (the P2P `MAX_PAYLOAD_SIZE`) the hex
//! conversion measured 285x the cost of the regex.
//!
//! **Correctness.** A hex string has two characters per byte, so a pattern can
//! match starting at an odd nibble. The bytes `07 6a 91 4X` encode to
//! `076a914X`, which contains `76a914` from offset 1 — so
//! `76a914[0-9a-f]{40}88ac` matches a script that is not P2PKH and contains no
//! `OP_DUP OP_HASH160` at all.
//!
//! This module translates the hex notation into an equivalent pattern over
//! bytes, which removes the encoding entirely and makes a misaligned match
//! structurally impossible: every element is required to consume whole bytes.
//!
//! # Supported notation
//!
//! | Hex notation | Meaning | Byte pattern |
//! |---|---|---|
//! | `76a914` | those literal bytes | `\x76\xa9\x14` |
//! | `[0-9a-f]{40}` | 40 hex chars = 20 bytes | `.{20}` |
//! | `[0-9a-f]*` | any number of bytes | `.*` |
//! | `[0-9a-f]+` | one or more bytes | `.+` |
//! | `^` / `$` | start / end of script | `^` / `$` |
//!
//! Anything else is **rejected**, not approximated. A pattern this module
//! cannot translate faithfully is a configuration error, and failing loudly is
//! better than silently monitoring the wrong thing.
//!
//! # Deliberate semantic changes
//!
//! * A match can no longer begin at an odd nibble. That is the point; any
//!   pattern that relied on it was matching by accident.
//! * `[0-9a-f]{N}` requires an even `N`, and `[0-9a-f]+` means one or more
//!   *bytes* rather than one or more hex characters. An odd count cannot
//!   describe a whole number of bytes.

use std::fmt;

use regex::bytes::{Regex, RegexBuilder};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HexPatternError {
    /// A literal run, or a `{N}` count, that does not describe whole bytes.
    NotByteAligned { at: usize, detail: String },
    /// Notation this module cannot translate faithfully.
    Unsupported { at: usize, detail: String },
    /// The translated pattern did not compile.
    Regex(String),
}

impl fmt::Display for HexPatternError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HexPatternError::NotByteAligned { at, detail } => write!(
                f,
                "locking script pattern is not byte aligned at offset {at}: {detail}"
            ),
            HexPatternError::Unsupported { at, detail } => write!(
                f,
                "unsupported locking script pattern at offset {at}: {detail}"
            ),
            HexPatternError::Regex(msg) => {
                write!(
                    f,
                    "translated locking script pattern did not compile: {msg}"
                )
            }
        }
    }
}

impl std::error::Error for HexPatternError {}

/// The only character class the notation accepts, in the spellings seen in
/// configuration. Both mean "one hex character".
const HEX_CLASSES: [&str; 2] = ["[0-9a-f]", "[0-9a-fA-F]"];

fn is_hex_digit(c: char) -> bool {
    c.is_ascii_hexdigit()
}

/// Translate a hex-notation pattern into a regex over raw script bytes.
pub fn compile(pattern: &str) -> Result<Regex, HexPatternError> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() * 2);
    let mut i = 0usize;

    while i < chars.len() {
        let c = chars[i];

        if c == '^' {
            // Only meaningful at the start; elsewhere regex would accept it but
            // it could not match, which is a silent dead pattern.
            if i != 0 {
                return Err(HexPatternError::Unsupported {
                    at: i,
                    detail: "'^' is only meaningful at the start of the pattern".to_string(),
                });
            }
            out.push('^');
            i += 1;
            continue;
        }

        if c == '$' {
            if i != chars.len() - 1 {
                return Err(HexPatternError::Unsupported {
                    at: i,
                    detail: "'$' is only meaningful at the end of the pattern".to_string(),
                });
            }
            out.push('$');
            i += 1;
            continue;
        }

        if is_hex_digit(c) {
            let start = i;
            while i < chars.len() && is_hex_digit(chars[i]) {
                i += 1;
            }
            let run: String = chars[start..i].iter().collect();
            if !run.len().is_multiple_of(2) {
                return Err(HexPatternError::NotByteAligned {
                    at: start,
                    detail: format!(
                        "literal run '{run}' is {} hex characters, which is not a whole number of bytes",
                        run.len()
                    ),
                });
            }
            // Emit every byte as an escape, so no byte can be read as a regex
            // metacharacter.
            for pair in run.as_bytes().chunks(2) {
                let hi = (pair[0] as char).to_digit(16).expect("checked hex digit");
                let lo = (pair[1] as char).to_digit(16).expect("checked hex digit");
                out.push_str(&format!("\\x{:02x}", hi * 16 + lo));
            }
            continue;
        }

        if c == '[' {
            let rest: String = chars[i..].iter().collect();
            let Some(class) = HEX_CLASSES.iter().find(|cls| rest.starts_with(**cls)) else {
                return Err(HexPatternError::Unsupported {
                    at: i,
                    detail: "the only supported character class is [0-9a-f]".to_string(),
                });
            };
            i += class.chars().count();

            // A bare class with no quantifier is one hex character, which is
            // half a byte and cannot be expressed.
            let Some(&next) = chars.get(i) else {
                return Err(HexPatternError::NotByteAligned {
                    at: i,
                    detail: "a character class with no quantifier matches a single hex character"
                        .to_string(),
                });
            };

            match next {
                '*' => {
                    out.push_str(".*");
                    i += 1;
                }
                '+' => {
                    out.push_str(".+");
                    i += 1;
                }
                '{' => {
                    let close = chars[i..].iter().position(|&c| c == '}').ok_or_else(|| {
                        HexPatternError::Unsupported {
                            at: i,
                            detail: "unterminated '{' quantifier".to_string(),
                        }
                    })? + i;
                    let count_str: String = chars[i + 1..close].iter().collect();
                    let count: usize =
                        count_str
                            .parse()
                            .map_err(|_| HexPatternError::Unsupported {
                                at: i,
                                detail: format!(
                                    "only a plain repetition count is supported, found '{{{count_str}}}'"
                                ),
                            })?;
                    if !count.is_multiple_of(2) {
                        return Err(HexPatternError::NotByteAligned {
                            at: i,
                            detail: format!(
                                "{count} hex characters is not a whole number of bytes"
                            ),
                        });
                    }
                    out.push_str(&format!(".{{{}}}", count / 2));
                    i = close + 1;
                }
                other => {
                    return Err(HexPatternError::NotByteAligned {
                        at: i,
                        detail: format!(
                            "a character class must be quantified to a whole number of bytes, found '{other}'"
                        ),
                    });
                }
            }
            continue;
        }

        return Err(HexPatternError::Unsupported {
            at: i,
            detail: format!("'{c}' is not supported in a locking script pattern"),
        });
    }

    // unicode(false) makes `.` a single byte rather than a UTF-8 sequence;
    // dot_matches_new_line(true) stops 0x0a being an accidental boundary.
    RegexBuilder::new(&out)
        .unicode(false)
        .dot_matches_new_line(true)
        .build()
        .map_err(|err| HexPatternError::Regex(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The patterns configured in data/uaasr.toml, by collection name.
    const LIVE_PATTERNS: [(&str, &str); 5] = [
        ("johns", "7576a914[0-9a-f]{40}88ac$"),
        ("dsa", "006a[0-9a-f]{2}53417631[0-9a-f]*"),
        ("CoCv1", "006a[0-9a-f]{2}436f437631[0-9a-f]*"),
        ("Fin", "76a914c0d164cbb336e3c64338c70506ef543c2fc7b8f988ac"),
        (
            "1sat",
            "0063036f726451126170706c69636174696f6e2f6273762d323000[0-9a-f]*",
        ),
    ];

    #[test]
    fn hex01_every_live_pattern_compiles() {
        for (name, pattern) in LIVE_PATTERNS {
            assert!(
                compile(pattern).is_ok(),
                "{name} should compile, got {:?}",
                compile(pattern)
            );
        }
    }

    #[test]
    fn hex02_literals_become_byte_escapes() {
        let re = compile("76a914").expect("compiles");
        assert!(re.is_match(&[0x76, 0xa9, 0x14]));
        assert!(re.is_match(&[0x00, 0x76, 0xa9, 0x14, 0xff]));
        assert!(!re.is_match(&[0x76, 0xa9, 0x15]));
    }

    #[test]
    fn hex03_counted_class_counts_bytes_not_hex_characters() {
        let re = compile("aa[0-9a-f]{4}bb").expect("compiles");
        // {4} hex characters == 2 bytes
        assert!(re.is_match(&[0xaa, 0x01, 0x02, 0xbb]));
        assert!(!re.is_match(&[0xaa, 0x01, 0xbb]));
        assert!(!re.is_match(&[0xaa, 0x01, 0x02, 0x03, 0xbb]));
    }

    #[test]
    fn hex04_star_and_plus_match_any_bytes_including_non_utf8() {
        let star = compile("aa[0-9a-f]*bb").expect("compiles");
        assert!(star.is_match(&[0xaa, 0xbb]));
        // 0xff and 0x0a are the cases a UTF-8 aware or line-oriented regex
        // would get wrong.
        assert!(star.is_match(&[0xaa, 0xff, 0x0a, 0x80, 0xbb]));

        let plus = compile("aa[0-9a-f]+bb").expect("compiles");
        assert!(!plus.is_match(&[0xaa, 0xbb]));
        assert!(plus.is_match(&[0xaa, 0x00, 0xbb]));
    }

    #[test]
    fn hex05_anchors_are_preserved() {
        let tail = compile("aabb$").expect("compiles");
        assert!(tail.is_match(&[0x11, 0xaa, 0xbb]));
        assert!(!tail.is_match(&[0xaa, 0xbb, 0x11]));

        let head = compile("^aabb").expect("compiles");
        assert!(head.is_match(&[0xaa, 0xbb, 0x11]));
        assert!(!head.is_match(&[0x11, 0xaa, 0xbb]));
    }

    // Anything that cannot be translated faithfully must be rejected, not
    // approximated. Silently monitoring the wrong thing is the worst outcome.
    #[test]
    fn hex06_untranslatable_notation_is_rejected() {
        for (pattern, why) in [
            ("76a91", "odd-length literal run"),
            ("[0-9a-f]{3}", "odd repetition count"),
            ("[0-9a-f]", "class with no quantifier"),
            ("[0-9a-f]?", "unsupported quantifier"),
            ("[0-9]{2}", "unsupported character class"),
            ("(76|a9)", "alternation"),
            ("76a914.*", "raw dot"),
            ("[0-9a-f]{2,4}", "range repetition"),
            ("aa$bb", "'$' not at the end"),
            ("aa^bb", "'^' not at the start"),
        ] {
            assert!(
                compile(pattern).is_err(),
                "'{pattern}' should be rejected ({why})"
            );
        }
    }

    // The correctness half of CS-402. A pattern must not match starting at an
    // odd nibble.
    #[test]
    fn hex07_nibble_misaligned_match_is_impossible() {
        let pattern = "76a914[0-9a-f]{40}88ac";

        // Shift a genuine p2pkh hex string by one nibble, then pad to a whole
        // number of bytes. The hex encoding still contains the pattern at
        // offset 1, but the bytes are not a p2pkh script.
        let shifted_hex = format!("0{}{}{}0", "76a914", "aa".repeat(20), "88ac");
        assert!(shifted_hex.len().is_multiple_of(2));
        let bytes = hex::decode(&shifted_hex).expect("valid hex");

        // The old matcher ran a text regex over exactly this string.
        let text_re = regex::Regex::new(pattern).expect("compiles");
        assert!(
            text_re.is_match(&shifted_hex),
            "precondition: the old hex-string matcher did match this"
        );

        let byte_re = compile(pattern).expect("compiles");
        assert!(
            !byte_re.is_match(&bytes),
            "a nibble-misaligned match must be impossible over bytes"
        );

        // And a genuine p2pkh script still matches.
        let genuine =
            hex::decode(format!("{}{}{}", "76a914", "aa".repeat(20), "88ac")).expect("valid hex");
        assert!(byte_re.is_match(&genuine));
    }

    // The strongest statement available: for every live pattern, over a corpus,
    // the byte matcher agrees with the old hex-string matcher on exactly the
    // byte-aligned matches, and differs only where the old one matched at an
    // odd nibble.
    #[test]
    fn hex08_agrees_with_the_hex_string_matcher_on_aligned_matches() {
        let corpus: Vec<Vec<u8>> = {
            let mut c: Vec<Vec<u8>> = Vec::new();
            // Genuine instances of each live pattern.
            c.push(hex::decode(format!("7576a914{}88ac", "11".repeat(20))).unwrap());
            c.push(hex::decode("006a0453417631deadbeef").unwrap());
            c.push(hex::decode("006a05436f43763100ff").unwrap());
            c.push(hex::decode("76a914c0d164cbb336e3c64338c70506ef543c2fc7b8f988ac").unwrap());
            c.push(
                hex::decode("0063036f726451126170706c69636174696f6e2f6273762d32320012").unwrap(),
            );
            // Near misses and noise.
            c.push(hex::decode(format!("76a914{}88ac", "22".repeat(20))).unwrap());
            c.push(hex::decode("006a").unwrap());
            c.push(hex::decode("00").unwrap());
            c.push(Vec::new());
            c.push((0u8..=255).collect());
            // Deliberately misaligned.
            c.push(hex::decode(format!("0{}{}{}0", "76a914", "aa".repeat(20), "88ac")).unwrap());
            c.push(hex::decode(format!("0{}0", "006a0453417631")).unwrap());
            c
        };

        for (name, pattern) in LIVE_PATTERNS {
            let byte_re = compile(pattern).expect("compiles");
            let text_re = regex::Regex::new(pattern).expect("compiles");

            for script in &corpus {
                let script_hex = hex::encode(script);
                let byte_match = byte_re.is_match(script);

                // Where did the old matcher match, if at all?
                let aligned_text_match = text_re
                    .find_iter(&script_hex)
                    .any(|m| m.start().is_multiple_of(2) && m.len().is_multiple_of(2));

                assert_eq!(
                    byte_match, aligned_text_match,
                    "{name}: byte matcher and byte-aligned hex matcher disagree on {script_hex}"
                );
            }
        }
    }
}

// Measures the cost this change removes. Skipped unless UAAS_BENCH is set, the
// same convention the database-backed tests use, because a 32 MB run has no
// place in every CI invocation. Meaningless in a debug build:
//
//   UAAS_BENCH=1 cargo test --release bench_probe -- --nocapture
#[cfg(test)]
mod bench_probe {
    use super::*;
    use crate::uaas::hexslice::HexSlice;
    use std::time::Instant;

    const PATTERN: &str = "76a914[0-9a-f]{40}88ac";

    fn script_of(size: usize) -> Vec<u8> {
        // Non-matching filler, so both matchers scan the whole thing.
        (0..size).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn bench_probe_hex_encoding_versus_byte_match() {
        if std::env::var("UAAS_BENCH").is_err() {
            eprintln!("skipping bench_probe: UAAS_BENCH not set");
            return;
        }
        if cfg!(debug_assertions) {
            eprintln!("bench_probe: debug build, numbers are not meaningful");
        }

        let text_re = regex::Regex::new(PATTERN).expect("compiles");
        let byte_re = compile(PATTERN).expect("compiles");

        println!(
            "\n{:>8} | {:>14} | {:>14} | {:>14} | {:>10}",
            "size", "HexSlice enc", "old total", "new total", "speedup"
        );
        println!(
            "{:->8}-+-{:->14}-+-{:->14}-+-{:->14}-+-{:->10}",
            "", "", "", "", ""
        );

        for mb in [1usize, 8, 32] {
            let script = script_of(mb * 1024 * 1024);

            // Old path: encode to hex, then run the text regex over the string.
            let t = Instant::now();
            let script_hex = format!("{}", HexSlice::new(&script));
            let encode_ms = t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            let old_hit = text_re.is_match(&script_hex);
            let old_total = encode_ms + t.elapsed().as_secs_f64() * 1000.0;

            // New path: match the bytes.
            let t = Instant::now();
            let new_hit = byte_re.is_match(&script);
            let new_total = t.elapsed().as_secs_f64() * 1000.0;

            assert_eq!(old_hit, new_hit, "the two paths must agree on {mb} MB");

            println!(
                "{:>6} MB | {:>11.2} ms | {:>11.2} ms | {:>11.2} ms | {:>9.0}x",
                mb,
                encode_ms,
                old_total,
                new_total,
                old_total / new_total.max(f64::MIN_POSITIVE)
            );
        }
        println!();
    }
}
