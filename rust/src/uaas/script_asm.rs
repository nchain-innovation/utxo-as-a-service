//! A script assembler for adversarial test fixtures.
//!
//! The corpus this supports is written as hex string literals today:
//!
//! ```text
//! let script = format!("0{}{}{}0", "76a914", H160, "88ac");
//! ```
//!
//! which is unreadable and miscounts nibbles silently. The same script here:
//!
//! ```text
//! OP_DUP OP_HASH160 0x7c78584493557fac782023a4ad591b64545929d9 OP_EQUALVERIFY OP_CHECKSIG
//! ```
//!
//! # Why this is not a general-purpose assembler
//!
//! A normal assembler picks the shortest encoding for a push. That makes the
//! cases this corpus exists to test unwritable, because roughly half of them
//! are about encoding equivalence — the same element under different push
//! opcodes must be expressible, and must round-trip to the exact bytes meant:
//!
//! ```text
//! 0x81                ->  01 81                 minimal, the default
//! OP_PUSHDATA1 0x81   ->  4c 01 81
//! OP_PUSHDATA2 0x81   ->  4d 01 00 81
//! OP_PUSHDATA4 0x81   ->  4e 01 00 00 00 81
//! OP_1NEGATE          ->  4f                    the numeric route to the same element
//! ```
//!
//! It also has to emit scripts that are *invalid*, for the tokeniser's error
//! paths — a push whose declared length exceeds the bytes that follow, a
//! truncated final push, an unknown opcode byte. Those have no well-formed
//! assembly syntax, so `raw:` emits bytes verbatim with no length prefix.
//!
//! # Opcode values come from `chain-gang`
//!
//! Only the *names* are written here; every byte value is taken from
//! `chain_gang::script::op_codes`. Hand-transcribing 104 opcode numbers to
//! build a tool whose purpose is to prevent hand-transcription errors would
//! have been self-defeating.
//!
//! Test-only: this module is `#[cfg(test)]` and is not compiled into the binary.

use chain_gang::script::op_codes;

/// Every opcode `chain-gang` names, mapped from its text form to its value.
/// Generated from `chain_gang::script::op_codes`; the values are that module's,
/// not copies of them.
#[rustfmt::skip]
const OPCODES: &[(&str, u8)] = &[
    ("OP_0", op_codes::OP_0),
    ("OP_FALSE", op_codes::OP_FALSE),
    ("OP_PUSH", op_codes::OP_PUSH),
    ("OP_PUSHDATA1", op_codes::OP_PUSHDATA1),
    ("OP_PUSHDATA2", op_codes::OP_PUSHDATA2),
    ("OP_PUSHDATA4", op_codes::OP_PUSHDATA4),
    ("OP_1NEGATE", op_codes::OP_1NEGATE),
    ("OP_1", op_codes::OP_1),
    ("OP_TRUE", op_codes::OP_TRUE),
    ("OP_2", op_codes::OP_2),
    ("OP_3", op_codes::OP_3),
    ("OP_4", op_codes::OP_4),
    ("OP_5", op_codes::OP_5),
    ("OP_6", op_codes::OP_6),
    ("OP_7", op_codes::OP_7),
    ("OP_8", op_codes::OP_8),
    ("OP_9", op_codes::OP_9),
    ("OP_10", op_codes::OP_10),
    ("OP_11", op_codes::OP_11),
    ("OP_12", op_codes::OP_12),
    ("OP_13", op_codes::OP_13),
    ("OP_14", op_codes::OP_14),
    ("OP_15", op_codes::OP_15),
    ("OP_16", op_codes::OP_16),
    ("OP_NOP", op_codes::OP_NOP),
    ("OP_IF", op_codes::OP_IF),
    ("OP_NOTIF", op_codes::OP_NOTIF),
    ("OP_ELSE", op_codes::OP_ELSE),
    ("OP_ENDIF", op_codes::OP_ENDIF),
    ("OP_VERIFY", op_codes::OP_VERIFY),
    ("OP_RETURN", op_codes::OP_RETURN),
    ("OP_TOALTSTACK", op_codes::OP_TOALTSTACK),
    ("OP_FROMALTSTACK", op_codes::OP_FROMALTSTACK),
    ("OP_IFDUP", op_codes::OP_IFDUP),
    ("OP_DEPTH", op_codes::OP_DEPTH),
    ("OP_DROP", op_codes::OP_DROP),
    ("OP_DUP", op_codes::OP_DUP),
    ("OP_NIP", op_codes::OP_NIP),
    ("OP_OVER", op_codes::OP_OVER),
    ("OP_PICK", op_codes::OP_PICK),
    ("OP_ROLL", op_codes::OP_ROLL),
    ("OP_ROT", op_codes::OP_ROT),
    ("OP_SWAP", op_codes::OP_SWAP),
    ("OP_TUCK", op_codes::OP_TUCK),
    ("OP_2DROP", op_codes::OP_2DROP),
    ("OP_2DUP", op_codes::OP_2DUP),
    ("OP_3DUP", op_codes::OP_3DUP),
    ("OP_2OVER", op_codes::OP_2OVER),
    ("OP_2ROT", op_codes::OP_2ROT),
    ("OP_2SWAP", op_codes::OP_2SWAP),
    ("OP_CAT", op_codes::OP_CAT),
    ("OP_SPLIT", op_codes::OP_SPLIT),
    ("OP_SUBSTR", op_codes::OP_SUBSTR),
    ("OP_LEFT", op_codes::OP_LEFT),
    ("OP_RIGHT", op_codes::OP_RIGHT),
    ("OP_SIZE", op_codes::OP_SIZE),
    ("OP_AND", op_codes::OP_AND),
    ("OP_OR", op_codes::OP_OR),
    ("OP_XOR", op_codes::OP_XOR),
    ("OP_EQUAL", op_codes::OP_EQUAL),
    ("OP_EQUALVERIFY", op_codes::OP_EQUALVERIFY),
    ("OP_1ADD", op_codes::OP_1ADD),
    ("OP_1SUB", op_codes::OP_1SUB),
    ("OP_NEGATE", op_codes::OP_NEGATE),
    ("OP_ABS", op_codes::OP_ABS),
    ("OP_NOT", op_codes::OP_NOT),
    ("OP_0NOTEQUAL", op_codes::OP_0NOTEQUAL),
    ("OP_ADD", op_codes::OP_ADD),
    ("OP_SUB", op_codes::OP_SUB),
    ("OP_2MUL", op_codes::OP_2MUL),
    ("OP_DIV", op_codes::OP_DIV),
    ("OP_2DIV", op_codes::OP_2DIV),
    ("OP_MOD", op_codes::OP_MOD),
    ("OP_BOOLAND", op_codes::OP_BOOLAND),
    ("OP_BOOLOR", op_codes::OP_BOOLOR),
    ("OP_NUMEQUAL", op_codes::OP_NUMEQUAL),
    ("OP_NUMEQUALVERIFY", op_codes::OP_NUMEQUALVERIFY),
    ("OP_NUMNOTEQUAL", op_codes::OP_NUMNOTEQUAL),
    ("OP_LESSTHAN", op_codes::OP_LESSTHAN),
    ("OP_GREATERTHAN", op_codes::OP_GREATERTHAN),
    ("OP_LESSTHANOREQUAL", op_codes::OP_LESSTHANOREQUAL),
    ("OP_GREATERTHANOREQUAL", op_codes::OP_GREATERTHANOREQUAL),
    ("OP_MIN", op_codes::OP_MIN),
    ("OP_MAX", op_codes::OP_MAX),
    ("OP_WITHIN", op_codes::OP_WITHIN),
    ("OP_NUM2BIN", op_codes::OP_NUM2BIN),
    ("OP_BIN2NUM", op_codes::OP_BIN2NUM),
    ("OP_RIPEMD160", op_codes::OP_RIPEMD160),
    ("OP_SHA1", op_codes::OP_SHA1),
    ("OP_SHA256", op_codes::OP_SHA256),
    ("OP_HASH160", op_codes::OP_HASH160),
    ("OP_HASH256", op_codes::OP_HASH256),
    ("OP_CODESEPARATOR", op_codes::OP_CODESEPARATOR),
    ("OP_CHECKSIG", op_codes::OP_CHECKSIG),
    ("OP_CHECKSIGVERIFY", op_codes::OP_CHECKSIGVERIFY),
    ("OP_CHECKMULTISIG", op_codes::OP_CHECKMULTISIG),
    ("OP_CHECKMULTISIGVERIFY", op_codes::OP_CHECKMULTISIGVERIFY),
    ("OP_CHECKLOCKTIMEVERIFY", op_codes::OP_CHECKLOCKTIMEVERIFY),
    ("OP_CHECKSEQUENCEVERIFY", op_codes::OP_CHECKSEQUENCEVERIFY),
    ("OP_VER", op_codes::OP_VER),
    ("OP_VERIF", op_codes::OP_VERIF),
    ("OP_VERNOTIF", op_codes::OP_VERNOTIF),
    ("OP_LSHIFTNUM", op_codes::OP_LSHIFTNUM),
    ("OP_RSHIFTNUM", op_codes::OP_RSHIFTNUM),
];

#[derive(Debug, PartialEq, Eq)]
pub enum AsmError {
    /// A token that is neither an opcode, a literal, nor a `raw:` escape.
    UnknownToken(String),
    /// A `0x` or `raw:` literal whose body is not hex, or has an odd digit count.
    BadHex { token: String, reason: &'static str },
    /// `OP_PUSHDATA{1,2,4}` not followed by a `0x` literal. Use `raw:` to emit a
    /// bare PUSHDATA opcode, which is a malformed script by construction.
    DanglingPushdata(&'static str),
    /// The element is longer than the requested encoding can describe.
    PushTooLarge { len: usize, encoding: &'static str },
    /// A decimal literal outside the range this assembler encodes.
    IntOutOfRange(i64),
}

/// Assembles script assembly text into the exact bytes it describes.
///
/// Tokens are whitespace-separated. `#` begins a comment to end of line.
///
/// | Token | Emits |
/// | --- | --- |
/// | `OP_DUP` | that opcode's byte |
/// | `0x<hex>` | a push of those bytes, shortest *push* encoding |
/// | `OP_PUSHDATA1 0x<hex>` | that push, forced into PUSHDATA1 (likewise 2, 4) |
/// | `<decimal>` | a minimal number push |
/// | `raw:<hex>` | those bytes verbatim, no length prefix |
///
/// A `0x` literal is a push of *exactly* those bytes; it is never rewritten
/// into `OP_1`..`OP_16` or `OP_1NEGATE`, because the distinction between
/// `0x81` and `OP_1NEGATE` is itself under test. Write the opcode when the
/// opcode is what is meant.
pub fn assemble(src: &str) -> Result<Vec<u8>, AsmError> {
    let mut out = Vec::new();
    let mut tokens = src
        .lines()
        .map(|line| line.split('#').next().unwrap_or(""))
        .flat_map(str::split_whitespace)
        .peekable();

    while let Some(token) = tokens.next() {
        // Forced push encodings consume the literal that follows them.
        let forced = match token {
            "OP_PUSHDATA1" => Some((1usize, "OP_PUSHDATA1")),
            "OP_PUSHDATA2" => Some((2usize, "OP_PUSHDATA2")),
            "OP_PUSHDATA4" => Some((4usize, "OP_PUSHDATA4")),
            _ => None,
        };
        if let Some((width, name)) = forced {
            // Only treat it as a forced push when a literal actually follows;
            // otherwise fall through and emit the bare opcode, so that
            // `OP_PUSHDATA1` alone is an error rather than a silent no-op.
            match tokens.peek() {
                Some(next) if next.starts_with("0x") => {
                    let data = parse_hex(tokens.next().expect("peeked"))?;
                    push_with_width(&mut out, &data, width, name)?;
                    continue;
                }
                _ => return Err(AsmError::DanglingPushdata(name)),
            }
        }

        if let Some(body) = token.strip_prefix("raw:") {
            out.extend_from_slice(&decode_hex(body, token)?);
            continue;
        }

        if token.starts_with("0x") {
            let data = parse_hex(token)?;
            push_minimal(&mut out, &data);
            continue;
        }

        if let Some(&(_, value)) = OPCODES.iter().find(|(name, _)| *name == token) {
            out.push(value);
            continue;
        }

        if let Ok(n) = token.parse::<i64>() {
            push_int(&mut out, n)?;
            continue;
        }

        return Err(AsmError::UnknownToken(token.to_string()));
    }

    Ok(out)
}

fn parse_hex(token: &str) -> Result<Vec<u8>, AsmError> {
    decode_hex(token.strip_prefix("0x").unwrap_or(token), token)
}

fn decode_hex(body: &str, token: &str) -> Result<Vec<u8>, AsmError> {
    if !body.len().is_multiple_of(2) {
        return Err(AsmError::BadHex {
            token: token.to_string(),
            reason: "odd number of hex digits",
        });
    }
    hex::decode(body).map_err(|_| AsmError::BadHex {
        token: token.to_string(),
        reason: "not hexadecimal",
    })
}

/// The shortest *push* encoding for these bytes. Deliberately does not
/// substitute `OP_1`..`OP_16` or `OP_1NEGATE` — see [`assemble`].
fn push_minimal(out: &mut Vec<u8>, data: &[u8]) {
    match data.len() {
        // A direct push of n bytes, for n <= 75, is the length itself.
        n if n <= 75 => out.push(n as u8),
        n if n <= u8::MAX as usize => {
            out.push(op_codes::OP_PUSHDATA1);
            out.push(n as u8);
        }
        n if n <= u16::MAX as usize => {
            out.push(op_codes::OP_PUSHDATA2);
            out.extend_from_slice(&(n as u16).to_le_bytes());
        }
        n => {
            out.push(op_codes::OP_PUSHDATA4);
            out.extend_from_slice(&(n as u32).to_le_bytes());
        }
    }
    out.extend_from_slice(data);
}

/// A push forced into a given PUSHDATA width, however short the element.
/// Lengths are little-endian, matching the wire format.
fn push_with_width(
    out: &mut Vec<u8>,
    data: &[u8],
    width: usize,
    encoding: &'static str,
) -> Result<(), AsmError> {
    let len = data.len();
    let too_large = match width {
        1 => len > u8::MAX as usize,
        2 => len > u16::MAX as usize,
        _ => len > u32::MAX as usize,
    };
    if too_large {
        return Err(AsmError::PushTooLarge { len, encoding });
    }

    match width {
        1 => {
            out.push(op_codes::OP_PUSHDATA1);
            out.push(len as u8);
        }
        2 => {
            out.push(op_codes::OP_PUSHDATA2);
            out.extend_from_slice(&(len as u16).to_le_bytes());
        }
        _ => {
            out.push(op_codes::OP_PUSHDATA4);
            out.extend_from_slice(&(len as u32).to_le_bytes());
        }
    }
    out.extend_from_slice(data);
    Ok(())
}

/// A decimal literal. Small values use the dedicated opcodes, as a real script
/// would; everything else becomes a minimal push of a sign-magnitude,
/// little-endian script number, where the high bit of the final byte is the
/// sign rather than part of the magnitude.
fn push_int(out: &mut Vec<u8>, n: i64) -> Result<(), AsmError> {
    match n {
        -1 => out.push(op_codes::OP_1NEGATE),
        0 => out.push(op_codes::OP_0),
        1..=16 => out.push(op_codes::OP_1 + (n as u8) - 1),
        _ => {
            let negative = n < 0;
            let mut magnitude = n.checked_abs().ok_or(AsmError::IntOutOfRange(n))? as u64;
            let mut bytes = Vec::new();
            while magnitude > 0 {
                bytes.push((magnitude & 0xff) as u8);
                magnitude >>= 8;
            }
            // The top bit of the last byte carries the sign, so a value that
            // already uses it needs a further byte to hold the sign alone.
            match bytes.last() {
                Some(&last) if last & 0x80 != 0 => bytes.push(if negative { 0x80 } else { 0x00 }),
                Some(_) if negative => {
                    let i = bytes.len() - 1;
                    bytes[i] |= 0x80;
                }
                _ => {}
            }
            push_minimal(out, &bytes);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const H160: &str = "7c78584493557fac782023a4ad591b64545929d9";

    fn asm(src: &str) -> Vec<u8> {
        assemble(src).expect("assembles")
    }

    // The first acceptance criterion: a P2PKH script, exactly 25 bytes.
    #[test]
    fn asm01_p2pkh_is_the_exact_25_bytes() {
        let bytes = asm(&format!(
            "OP_DUP OP_HASH160 0x{H160} OP_EQUALVERIFY OP_CHECKSIG"
        ));
        assert_eq!(bytes.len(), 25);
        assert_eq!(hex::encode(&bytes), format!("76a914{H160}88ac"));
    }

    // Every encoding of the element 0x81 must be writable and produce exactly
    // the bytes intended. This is the criterion the whole module exists for:
    // PUSHDATA2 and PUSHDATA4 are currently false negatives in the matcher, and
    // without this they cannot even be expressed as a fixture.
    #[test]
    fn asm02_all_five_encodings_of_0x81() {
        assert_eq!(asm("0x81"), vec![0x01, 0x81], "minimal direct push");
        assert_eq!(asm("OP_PUSHDATA1 0x81"), vec![0x4c, 0x01, 0x81]);
        assert_eq!(asm("OP_PUSHDATA2 0x81"), vec![0x4d, 0x01, 0x00, 0x81]);
        assert_eq!(
            asm("OP_PUSHDATA4 0x81"),
            vec![0x4e, 0x01, 0x00, 0x00, 0x00, 0x81]
        );
        // The numeric route to the same element on the stack: -1 is 0x81 in
        // script's sign-magnitude form.
        assert_eq!(asm("OP_1NEGATE"), vec![0x4f]);
        assert_eq!(asm("-1"), vec![0x4f]);
    }

    // A `0x` literal is a push of those exact bytes and must never be rewritten
    // into a numeric opcode, or the distinction under test disappears.
    #[test]
    fn asm03_hex_literals_are_never_rewritten_as_numeric_opcodes() {
        assert_eq!(asm("0x01"), vec![0x01, 0x01], "not OP_1");
        assert_eq!(asm("0x10"), vec![0x01, 0x10], "not OP_16");
        assert_eq!(asm("0x81"), vec![0x01, 0x81], "not OP_1NEGATE");
        // Written as opcodes, they are the opcodes.
        assert_eq!(asm("1"), vec![0x51]);
        assert_eq!(asm("16"), vec![0x60]);
    }

    #[test]
    fn asm04_minimal_push_boundaries() {
        // 75 bytes is the largest direct push; 76 needs PUSHDATA1.
        let d75 = vec![0xab; 75];
        let out = asm(&format!("0x{}", hex::encode(&d75)));
        assert_eq!(out[0], 75);
        assert_eq!(out.len(), 76);

        let d76 = vec![0xab; 76];
        let out = asm(&format!("0x{}", hex::encode(&d76)));
        assert_eq!(&out[..2], &[0x4c, 76]);
        assert_eq!(out.len(), 78);

        // 256 bytes no longer fits a u8 length, so PUSHDATA2, little-endian.
        let d256 = vec![0xcd; 256];
        let out = asm(&format!("0x{}", hex::encode(&d256)));
        assert_eq!(&out[..3], &[0x4d, 0x00, 0x01]);
        assert_eq!(out.len(), 259);
    }

    #[test]
    fn asm05_empty_push() {
        assert_eq!(asm("0x"), vec![0x00]);
    }

    // Malformed scripts, which have no well-formed assembly syntax. These are
    // the tokeniser's error paths.
    #[test]
    fn asm06_raw_escapes_emit_bytes_verbatim() {
        // A PUSHDATA1 declaring five bytes with only two following.
        assert_eq!(
            asm("raw:4c05 raw:0102"),
            vec![0x4c, 0x05, 0x01, 0x02],
            "declared length exceeds the bytes present"
        );
        // A truncated final push: says 10 bytes, supplies one.
        assert_eq!(asm("raw:0aff"), vec![0x0a, 0xff]);
        // An opcode byte chain-gang does not name.
        assert_eq!(asm("raw:ff"), vec![0xff]);
        // Raw bytes mix with assembled ones.
        assert_eq!(
            asm("OP_RETURN raw:4c05 0x01"),
            vec![0x6a, 0x4c, 0x05, 0x01, 0x01]
        );
    }

    #[test]
    fn asm07_comments_and_whitespace() {
        let bytes = asm("
            OP_DUP OP_HASH160     # hash the pubkey
            0x7c78584493557fac782023a4ad591b64545929d9
            OP_EQUALVERIFY OP_CHECKSIG   # and check it
        ");
        assert_eq!(hex::encode(&bytes), format!("76a914{H160}88ac"));
    }

    #[test]
    fn asm08_opcode_values_come_from_chain_gang() {
        // Spot-check against chain-gang rather than against numbers written
        // here, which is the whole point of sourcing the table from it.
        assert_eq!(asm("OP_DUP"), vec![op_codes::OP_DUP]);
        assert_eq!(asm("OP_RETURN"), vec![op_codes::OP_RETURN]);
        assert_eq!(asm("OP_NOTIF"), vec![op_codes::OP_NOTIF]);
        assert_eq!(asm("OP_CHECKMULTISIG"), vec![op_codes::OP_CHECKMULTISIG]);
        // Aliases resolve to the same byte.
        assert_eq!(asm("OP_FALSE"), asm("OP_0"));
        assert_eq!(asm("OP_TRUE"), asm("OP_1"));
    }

    #[test]
    fn asm09_errors_are_specific() {
        assert_eq!(
            assemble("OP_NOT_A_REAL_OPCODE"),
            Err(AsmError::UnknownToken("OP_NOT_A_REAL_OPCODE".to_string()))
        );
        assert!(matches!(
            assemble("0xabc"),
            Err(AsmError::BadHex {
                reason: "odd number of hex digits",
                ..
            })
        ));
        assert!(matches!(
            assemble("0xzz"),
            Err(AsmError::BadHex {
                reason: "not hexadecimal",
                ..
            })
        ));
        // A bare PUSHDATA is a malformed script; say so rather than emitting
        // something that looks deliberate. `raw:4c` is how to mean it.
        assert_eq!(
            assemble("OP_PUSHDATA1"),
            Err(AsmError::DanglingPushdata("OP_PUSHDATA1"))
        );
        assert_eq!(
            assemble("OP_PUSHDATA1 OP_DUP"),
            Err(AsmError::DanglingPushdata("OP_PUSHDATA1"))
        );
    }

    #[test]
    fn asm10_pushdata1_rejects_an_element_it_cannot_describe() {
        let big = hex::encode(vec![0u8; 256]);
        assert_eq!(
            assemble(&format!("OP_PUSHDATA1 0x{big}")),
            Err(AsmError::PushTooLarge {
                len: 256,
                encoding: "OP_PUSHDATA1"
            })
        );
    }

    // Script numbers are sign-magnitude little-endian, not two's complement:
    // the high bit of the final byte is the sign.
    #[test]
    fn asm11_script_number_encoding() {
        assert_eq!(asm("17"), vec![0x01, 0x11]);
        assert_eq!(asm("127"), vec![0x01, 0x7f]);
        // 128 would set the sign bit, so it needs a byte for the sign alone.
        assert_eq!(asm("128"), vec![0x02, 0x80, 0x00]);
        assert_eq!(asm("255"), vec![0x02, 0xff, 0x00]);
        assert_eq!(asm("256"), vec![0x02, 0x00, 0x01]);
        // Negatives set the top bit of the final byte.
        assert_eq!(asm("-2"), vec![0x01, 0x82]);
        assert_eq!(asm("-127"), vec![0x01, 0xff]);
        assert_eq!(asm("-128"), vec![0x02, 0x80, 0x80]);
    }

    // The fixtures the existing corpus needs, assembled rather than hand-typed.
    // Probe H acquired a nibble-miscount bug during the review precisely here.
    #[test]
    fn asm12_probe_shapes_assemble_to_the_intended_bytes() {
        // An OP_RETURN carrying a payload, as probes A and B need.
        let payload = hex::encode(b"application/bsv-20");
        let bytes = asm(&format!("OP_FALSE OP_RETURN 0x{payload}"));
        assert_eq!(bytes[0], op_codes::OP_0);
        assert_eq!(bytes[1], op_codes::OP_RETURN);
        assert_eq!(bytes[2], 18, "length prefix for an 18-byte element");
        assert_eq!(&bytes[3..], b"application/bsv-20");

        // A key consumed by OP_DROP, never reaching a CHECKSIG: probe C.
        let key = hex::encode([0x02u8; 33]);
        let bytes = asm(&format!("0x{key} OP_DROP OP_TRUE"));
        assert_eq!(bytes[0], 33);
        assert_eq!(bytes[34], op_codes::OP_DROP);
        assert_eq!(bytes[35], op_codes::OP_TRUE);

        // OP_RETURN inside a branch, as probe B needs.
        let bytes = asm("OP_IF OP_RETURN OP_ELSE OP_TRUE OP_ENDIF");
        assert_eq!(
            bytes,
            vec![
                op_codes::OP_IF,
                op_codes::OP_RETURN,
                op_codes::OP_ELSE,
                op_codes::OP_TRUE,
                op_codes::OP_ENDIF
            ]
        );
    }

    // The same 33-byte element under all four push encodings — probe D's
    // subject. Each must carry an identical payload with a different prefix.
    #[test]
    fn asm13_same_element_under_every_push_encoding() {
        let key = hex::encode([0x03u8; 33]);
        let minimal = asm(&format!("0x{key}"));
        let p1 = asm(&format!("OP_PUSHDATA1 0x{key}"));
        let p2 = asm(&format!("OP_PUSHDATA2 0x{key}"));
        let p4 = asm(&format!("OP_PUSHDATA4 0x{key}"));

        assert_eq!(&minimal[1..], &p1[2..]);
        assert_eq!(&minimal[1..], &p2[3..]);
        assert_eq!(&minimal[1..], &p4[5..]);

        assert_eq!(minimal.len(), 34);
        assert_eq!(p1.len(), 35);
        assert_eq!(p2.len(), 36);
        assert_eq!(p4.len(), 38);
    }
}
