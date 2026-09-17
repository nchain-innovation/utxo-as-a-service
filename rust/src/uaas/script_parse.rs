//! Tokenise a locking script into opcode boundaries, push elements and branch
//! structure.
//!
//! The collection matcher is a substring search over bytes. It has no notion of
//! where one opcode ends and the next begins, which execution paths are
//! reachable, or where script stops and data starts — so it cannot distinguish
//! "this output pays the monitored key" from "these bytes appear somewhere in
//! this output". No regex can acquire those notions, because they are not
//! properties of the byte string. This module is the prerequisite for the
//! matcher that can (CS-415).
//!
//! # What it does and does not do
//!
//! Parse only. No execution, no stack, no matching, no signature analysis.
//! The tokeniser reports what the bytes *are*; deciding what that means is the
//! caller's job.
//!
//! # Input is unbounded and hostile
//!
//! Post-Genesis there is no 10,000-byte script cap, no 520-byte element cap and
//! no stack element count cap, and Chronicle raised the maximum script number
//! length to 32 MB. A locking script arrives inside a transaction from the P2P
//! network, so every byte is chosen by whoever built it. Three consequences
//! shape the design:
//!
//! **It allocates nothing.** [`tokenise`] returns a lazy iterator, and every
//! element is a borrowed subslice of the input rather than a copy. Collecting
//! the tokens is the caller's choice, and a caller that only wants to walk the
//! script never pays for it. A 32 MB script of single-byte opcodes would be 32
//! million tokens; materialising that unasked would be a denial of service in
//! its own right.
//!
//! **It never allocates against a declared length.** A push declaring four
//! gigabytes is checked against the bytes that actually remain before anything
//! reads them. That is the CS-395 lesson: an allocation failure in Rust aborts
//! the process rather than unwinding, so it cannot be caught.
//!
//! **It is iterative.** Branch nesting is a counter, not recursion. Ten
//! thousand nested `OP_IF`s cost one `u32`. A stack overflow is also an abort,
//! not a catchable panic.
//!
//! It never panics and contains no `unwrap` or `expect`.
//!
//! # Malformed input yields a marker, not an error
//!
//! A script that fails to parse cleanly is still a script that a miner will
//! execute, so refusing to describe it is the wrong answer. The iterator yields
//! the tokens it could read and finishes with [`TokenKind::Truncated`] saying
//! where it stopped and why.
//!
//! # Byte order
//!
//! `OP_PUSHDATA1/2/4` length fields are little-endian, which is the only
//! multi-byte encoding in the token grammar.

use chain_gang::script::op_codes;

/// The largest element a single direct push opcode can describe. Opcodes
/// `0x01..=0x4b` *are* the length; `0x4c` is `OP_PUSHDATA1`.
const MAX_DIRECT_PUSH: u8 = op_codes::OP_PUSHDATA1 - 1;

/// `OP_1`..`OP_16` push the numbers 1..16 as one-byte elements.
static SMALL_INT_ELEMENTS: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

/// `OP_1NEGATE` pushes -1, which in sign-magnitude little-endian script number
/// encoding is the single byte `0x81`.
static ONE_NEGATE_ELEMENT: [u8; 1] = [0x81];

/// How a push was written, as distinct from what it pushed.
///
/// The two are separate on purpose. An element is the same element however it
/// was encoded, which is what a matcher must compare; but the encoding is the
/// difference between a standard script and one built to evade a substring
/// matcher, and it is what re-serialisation needs. Collapsing them would make
/// the token stream lossy in exactly the direction an attacker chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushEncoding {
    /// `OP_0` — the empty element.
    Op0,
    /// `OP_1NEGATE`.
    OneNegate,
    /// `OP_1`..`OP_16`, carrying the opcode byte.
    SmallInt(u8),
    /// The opcode byte is itself the length, `0x01..=0x4b`.
    Direct,
    /// `OP_PUSHDATA1`, one length byte.
    PushData1,
    /// `OP_PUSHDATA2`, two length bytes, little-endian.
    PushData2,
    /// `OP_PUSHDATA4`, four length bytes, little-endian.
    PushData4,
}

/// The branch opcodes, kept separate from [`TokenKind::Op`] because branch
/// structure is the thing a matcher has to understand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchOp {
    If,
    NotIf,
    Else,
    EndIf,
}

/// Why the walk stopped early.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Truncation {
    /// A `OP_PUSHDATA{1,2,4}` whose own length field runs off the end.
    LengthFieldCutOff {
        at: usize,
        needed: usize,
        remaining: usize,
    },
    /// A push declaring more bytes than the script has left. `declared` is a
    /// `u64` because `OP_PUSHDATA4` can name a length no `usize` need hold.
    PushBeyondEnd {
        at: usize,
        declared: u64,
        remaining: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind<'a> {
    /// A push, with the element exposed independently of its encoding.
    Push {
        encoding: PushEncoding,
        element: &'a [u8],
    },
    /// `OP_IF` / `OP_NOTIF` / `OP_ELSE` / `OP_ENDIF`.
    Branch(BranchOp),
    /// `OP_RETURN`. At depth 0 this ends the executable script; deeper it does
    /// not, because the branch containing it may not be taken.
    Return,
    /// Any other opcode byte, including the ones no implementation defines.
    Op(u8),
    /// Everything after a top-level `OP_RETURN`. Never executed, never parsed.
    TrailingData(&'a [u8]),
    /// The walk could not continue. Always the last token.
    Truncated(Truncation),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: TokenKind<'a>,
    /// Byte offset of this token's first byte in the script.
    pub offset: usize,
    /// Number of enclosing `OP_IF`/`OP_NOTIF` constructs.
    ///
    /// A branch opcode reports the depth *outside* the construct it opens,
    /// divides or closes, so `OP_IF`, its matching `OP_ELSE` and its matching
    /// `OP_ENDIF` all carry the same depth, and the tokens between them carry
    /// one more.
    pub depth: u32,
}

/// Walks `script`, yielding one token per opcode.
///
/// Allocates nothing; every element borrows from `script`.
pub fn tokenise(script: &[u8]) -> Tokens<'_> {
    Tokens {
        script,
        pos: 0,
        depth: 0,
        finished: false,
        pending_trailing: false,
    }
}

pub struct Tokens<'a> {
    script: &'a [u8],
    pos: usize,
    depth: u32,
    /// Set once a terminal token has been yielded, so nothing follows it.
    finished: bool,
    /// Set when a top-level `OP_RETURN` has been yielded. The next call emits
    /// the remaining bytes as data and ends the walk.
    pending_trailing: bool,
}

impl<'a> Tokens<'a> {
    /// Reads a little-endian length of `width` bytes at `self.pos`.
    ///
    /// Returns `None` when the field itself is cut off. Widths are 1, 2 and 4,
    /// so the accumulation cannot overflow `u64`.
    fn read_len(&mut self, width: usize) -> Option<u64> {
        let bytes = self.script.get(self.pos..self.pos.checked_add(width)?)?;
        let mut len: u64 = 0;
        for (i, byte) in bytes.iter().enumerate() {
            // Little-endian: the first byte is least significant.
            len |= u64::from(*byte) << (8 * i);
        }
        self.pos += width;
        Some(len)
    }

    /// Takes `declared` bytes as a push element, or reports the truncation.
    ///
    /// The comparison happens in `u64` before any cast to `usize`, so a
    /// declared length larger than the address space is rejected rather than
    /// wrapped — and nothing is read, let alone allocated, until it passes.
    fn take_element(
        &mut self,
        start: usize,
        declared: u64,
        encoding: PushEncoding,
    ) -> TokenKind<'a> {
        let remaining = self.script.len().saturating_sub(self.pos);
        if declared > remaining as u64 {
            self.finished = true;
            return TokenKind::Truncated(Truncation::PushBeyondEnd {
                at: start,
                declared,
                remaining,
            });
        }
        // Safe: declared <= remaining, and remaining is a usize.
        let len = declared as usize;
        let element = &self.script[self.pos..self.pos + len];
        self.pos += len;
        TokenKind::Push { encoding, element }
    }

    fn length_cut_off(&mut self, start: usize, needed: usize) -> TokenKind<'a> {
        self.finished = true;
        TokenKind::Truncated(Truncation::LengthFieldCutOff {
            at: start,
            needed,
            remaining: self.script.len().saturating_sub(self.pos),
        })
    }
}

impl<'a> Iterator for Tokens<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Token<'a>> {
        if self.finished {
            return None;
        }
        if self.pending_trailing {
            self.pending_trailing = false;
            self.finished = true;
            let start = self.pos;
            let data = &self.script[start..];
            self.pos = self.script.len();
            return Some(Token {
                kind: TokenKind::TrailingData(data),
                offset: start,
                depth: 0,
            });
        }
        let start = self.pos;
        let Some(&opcode) = self.script.get(start) else {
            self.finished = true;
            return None;
        };
        self.pos += 1;

        // Depth is reported for the block the token belongs to, so a branch
        // opcode names the depth outside the construct it acts on. Saturating
        // rather than wrapping: a script can nest deeper than u32::MAX only by
        // being larger than any script will be, and saturating keeps the
        // reported nesting monotonic instead of wrapping to zero.
        let (kind, depth) = match opcode {
            op_codes::OP_0 => (
                TokenKind::Push {
                    encoding: PushEncoding::Op0,
                    element: &[],
                },
                self.depth,
            ),

            1..=MAX_DIRECT_PUSH => (
                self.take_element(start, u64::from(opcode), PushEncoding::Direct),
                self.depth,
            ),

            op_codes::OP_PUSHDATA1 => match self.read_len(1) {
                Some(len) => (
                    self.take_element(start, len, PushEncoding::PushData1),
                    self.depth,
                ),
                None => (self.length_cut_off(start, 1), self.depth),
            },
            op_codes::OP_PUSHDATA2 => match self.read_len(2) {
                Some(len) => (
                    self.take_element(start, len, PushEncoding::PushData2),
                    self.depth,
                ),
                None => (self.length_cut_off(start, 2), self.depth),
            },
            op_codes::OP_PUSHDATA4 => match self.read_len(4) {
                Some(len) => (
                    self.take_element(start, len, PushEncoding::PushData4),
                    self.depth,
                ),
                None => (self.length_cut_off(start, 4), self.depth),
            },

            op_codes::OP_1NEGATE => (
                TokenKind::Push {
                    encoding: PushEncoding::OneNegate,
                    element: &ONE_NEGATE_ELEMENT,
                },
                self.depth,
            ),

            op_codes::OP_1..=op_codes::OP_16 => {
                // OP_1 is 1, so the index is the opcode's distance from it.
                let index = usize::from(opcode - op_codes::OP_1);
                (
                    TokenKind::Push {
                        encoding: PushEncoding::SmallInt(opcode),
                        element: &SMALL_INT_ELEMENTS[index..index + 1],
                    },
                    self.depth,
                )
            }

            op_codes::OP_IF | op_codes::OP_NOTIF => {
                let outer = self.depth;
                self.depth = self.depth.saturating_add(1);
                let op = if opcode == op_codes::OP_IF {
                    BranchOp::If
                } else {
                    BranchOp::NotIf
                };
                (TokenKind::Branch(op), outer)
            }
            op_codes::OP_ELSE => {
                // An OP_ELSE with no OP_IF open is unbalanced. Reporting depth
                // 0 is the honest answer; the caller decides what to make of it.
                (
                    TokenKind::Branch(BranchOp::Else),
                    self.depth.saturating_sub(1),
                )
            }
            op_codes::OP_ENDIF => {
                self.depth = self.depth.saturating_sub(1);
                (TokenKind::Branch(BranchOp::EndIf), self.depth)
            }

            op_codes::OP_RETURN => (TokenKind::Return, self.depth),

            other => (TokenKind::Op(other), self.depth),
        };

        // A top-level OP_RETURN ends the executable script: everything after it
        // is data and must not be read as opcodes. Inside a branch it does not,
        // because whether that branch executes is a runtime property this
        // module deliberately does not model. "The first OP_RETURN in the byte
        // stream" is therefore the wrong boundary, and choosing it is how a
        // matcher gets fooled by a script like `OP_IF OP_RETURN OP_ENDIF ...`.
        if matches!(kind, TokenKind::Return) && depth == 0 {
            self.pending_trailing = true;
        }

        Some(Token {
            kind,
            offset: start,
            depth,
        })
    }
}

impl PushEncoding {
    /// Re-serialises a push to the exact bytes it was read from.
    ///
    /// Together with [`Token::write_to`] this is what makes the token stream
    /// demonstrably lossless: the round-trip is reconstructed from the
    /// `(encoding, element)` pair alone, never from a retained copy of the
    /// input, so a token that dropped information cannot round-trip.
    pub fn write_to(&self, element: &[u8], out: &mut Vec<u8>) {
        match self {
            PushEncoding::Op0 => out.push(op_codes::OP_0),
            PushEncoding::OneNegate => out.push(op_codes::OP_1NEGATE),
            PushEncoding::SmallInt(opcode) => out.push(*opcode),
            PushEncoding::Direct => {
                // Only produced for lengths 1..=MAX_DIRECT_PUSH, so the cast
                // cannot truncate.
                out.push(element.len() as u8);
                out.extend_from_slice(element);
            }
            PushEncoding::PushData1 => {
                out.push(op_codes::OP_PUSHDATA1);
                out.push(element.len() as u8);
                out.extend_from_slice(element);
            }
            PushEncoding::PushData2 => {
                out.push(op_codes::OP_PUSHDATA2);
                out.extend_from_slice(&(element.len() as u16).to_le_bytes());
                out.extend_from_slice(element);
            }
            PushEncoding::PushData4 => {
                out.push(op_codes::OP_PUSHDATA4);
                out.extend_from_slice(&(element.len() as u32).to_le_bytes());
                out.extend_from_slice(element);
            }
        }
    }
}

impl Token<'_> {
    /// Appends this token's original bytes to `out`.
    ///
    /// A [`TokenKind::Truncated`] token writes nothing: the bytes it describes
    /// are the ones that were not there.
    pub fn write_to(&self, out: &mut Vec<u8>) {
        match self.kind {
            TokenKind::Push { encoding, element } => encoding.write_to(element, out),
            TokenKind::Branch(BranchOp::If) => out.push(op_codes::OP_IF),
            TokenKind::Branch(BranchOp::NotIf) => out.push(op_codes::OP_NOTIF),
            TokenKind::Branch(BranchOp::Else) => out.push(op_codes::OP_ELSE),
            TokenKind::Branch(BranchOp::EndIf) => out.push(op_codes::OP_ENDIF),
            TokenKind::Return => out.push(op_codes::OP_RETURN),
            TokenKind::Op(byte) => out.push(byte),
            TokenKind::TrailingData(data) => out.extend_from_slice(data),
            TokenKind::Truncated(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uaas::script_asm::assemble;

    fn asm(src: &str) -> Vec<u8> {
        assemble(src).unwrap_or_else(|err| panic!("fixture must assemble: {src:?}: {err:?}"))
    }

    fn kinds(script: &[u8]) -> Vec<TokenKind<'_>> {
        tokenise(script).map(|t| t.kind).collect()
    }

    /// Re-serialise a token stream from `(encoding, element)` alone.
    fn reassemble(script: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for token in tokenise(script) {
            token.write_to(&mut out);
        }
        out
    }

    fn elements(script: &[u8]) -> Vec<&[u8]> {
        tokenise(script)
            .filter_map(|t| match t.kind {
                TokenKind::Push { element, .. } => Some(element),
                _ => None,
            })
            .collect()
    }

    // The headline criterion. Five ways to put the same element on the stack;
    // a matcher that compares elements must see one element, not five.
    #[test]
    fn parse01_all_five_encodings_of_one_negative_yield_the_same_element() {
        let sources = [
            "OP_1NEGATE",
            "0x81",
            "OP_PUSHDATA1 0x81",
            "OP_PUSHDATA2 0x81",
            "OP_PUSHDATA4 0x81",
        ];

        for src in sources {
            let script = asm(src);
            assert_eq!(
                elements(&script),
                vec![&[0x81u8][..]],
                "{src} must yield the element 0x81"
            );
        }

        // And the encodings are genuinely different bytes, so the agreement
        // above is the tokeniser's doing and not the assembler collapsing them.
        let encodings: Vec<Vec<u8>> = sources.iter().map(|src| asm(src)).collect();
        for (i, a) in encodings.iter().enumerate() {
            for b in encodings.iter().skip(i + 1) {
                assert_ne!(a, b, "the fixtures must differ as bytes");
            }
        }
    }

    // The same, for an element that cannot be written as a numeric opcode:
    // the 33-byte key from probe D.
    #[test]
    fn parse02_all_four_push_encodings_of_a_key_yield_the_same_element() {
        const KEY: &str = "02111111111111111111111111111111111111111111111111111111111111111e";
        let expected = hex::decode(KEY).expect("key hex");

        for src in [
            format!("0x{KEY}"),
            format!("OP_PUSHDATA1 0x{KEY}"),
            format!("OP_PUSHDATA2 0x{KEY}"),
            format!("OP_PUSHDATA4 0x{KEY}"),
        ] {
            let script = asm(&src);
            assert_eq!(elements(&script), vec![expected.as_slice()], "{src}");
        }
    }

    #[test]
    fn parse03_small_integer_opcodes_are_pushes() {
        let script = asm("OP_0 OP_1 OP_2 OP_16");
        assert_eq!(
            elements(&script),
            vec![&[][..], &[1u8][..], &[2u8][..], &[16u8][..]]
        );

        // OP_0 is the empty element, not a zero byte. A matcher that confused
        // the two would match an OP_0 against a pushed 0x00.
        let zero_byte = asm("0x00");
        assert_eq!(elements(&zero_byte), vec![&[0x00u8][..]]);
        assert_ne!(elements(&script)[0], elements(&zero_byte)[0]);
    }

    #[test]
    fn parse04_p2pkh_tokenises_to_its_five_operations() {
        let script = asm("OP_DUP OP_HASH160 0xaabbccddeeff00112233445566778899aabbccdd OP_EQUALVERIFY OP_CHECKSIG");
        let kinds = kinds(&script);
        assert_eq!(kinds.len(), 5, "p2pkh is five tokens, got {kinds:?}");
        assert!(matches!(kinds[2], TokenKind::Push { .. }));
        assert_eq!(
            tokenise(&script).map(|t| t.offset).collect::<Vec<_>>(),
            vec![0, 1, 2, 23, 24],
            "offsets must name the opcode boundaries"
        );
    }

    // Branch nesting is a counter, and the depth reported for a branch opcode
    // is the depth outside the construct it acts on.
    #[test]
    fn parse05_branch_depth_is_reported_for_the_enclosing_block() {
        let script = asm("OP_IF OP_1 OP_IF OP_2 OP_ELSE OP_3 OP_ENDIF OP_ENDIF OP_4");
        let depths: Vec<u32> = tokenise(&script).map(|t| t.depth).collect();
        //                     IF 1  IF 2  ELSE 3  ENDIF ENDIF 4
        assert_eq!(depths, vec![0, 1, 1, 2, 1, 2, 1, 0, 0]);
    }

    // The probe B shape. A tokeniser that took the first OP_RETURN in the byte
    // stream as the script/data boundary would stop here and call the rest
    // data, which is how the substring matcher is fooled.
    #[test]
    fn parse06_op_return_inside_a_branch_does_not_end_the_script() {
        let script = asm("OP_IF OP_RETURN OP_ENDIF 0xaabb OP_CHECKSIG");
        let kinds = kinds(&script);

        assert!(
            !kinds
                .iter()
                .any(|k| matches!(k, TokenKind::TrailingData(_))),
            "a branched OP_RETURN must not open a trailing-data section: {kinds:?}"
        );
        assert_eq!(
            kinds,
            vec![
                TokenKind::Branch(BranchOp::If),
                TokenKind::Return,
                TokenKind::Branch(BranchOp::EndIf),
                TokenKind::Push {
                    encoding: PushEncoding::Direct,
                    element: &[0xaa, 0xbb],
                },
                TokenKind::Op(op_codes::OP_CHECKSIG),
            ]
        );

        // The OP_RETURN is at depth 1, which is what distinguishes it.
        let return_depth = tokenise(&script)
            .find(|t| matches!(t.kind, TokenKind::Return))
            .map(|t| t.depth);
        assert_eq!(return_depth, Some(1));
    }

    // The probe A shape. A top-level OP_RETURN does end the script, and what
    // follows is data that must not be read as opcodes.
    #[test]
    fn parse07_top_level_op_return_opens_trailing_data() {
        let script = asm("OP_RETURN 0x76a914aabbccddeeff00112233445566778899aabbccdd88ac");
        let kinds = kinds(&script);

        assert_eq!(kinds.len(), 2, "OP_RETURN then everything else: {kinds:?}");
        assert_eq!(kinds[0], TokenKind::Return);
        match kinds[1] {
            TokenKind::TrailingData(data) => {
                assert_eq!(data, &script[1..]);
                // The bytes look exactly like a p2pkh script. They are not one,
                // and the tokeniser must not have parsed them as opcodes.
                assert_eq!(data[0], 25, "a 25-byte push, unparsed");
            }
            ref other => panic!("expected trailing data, got {other:?}"),
        }
    }

    #[test]
    fn parse08_trailing_data_may_be_empty() {
        let script = asm("OP_RETURN");
        assert_eq!(
            kinds(&script),
            vec![TokenKind::Return, TokenKind::TrailingData(&[])]
        );
    }

    // Every malformed shape returns a marker. None of these may panic, and the
    // PUSHDATA4 case must not allocate against its declared length.
    #[test]
    fn parse09_malformed_input_yields_a_truncation_marker() {
        // A push declaring 4 GiB with nothing behind it.
        let script = asm("raw:4effffffff");
        assert_eq!(
            kinds(&script),
            vec![TokenKind::Truncated(Truncation::PushBeyondEnd {
                at: 0,
                declared: 0xffff_ffff,
                remaining: 0,
            })]
        );

        // A direct push declaring more than remains.
        let script = asm("raw:20aabb");
        assert_eq!(
            kinds(&script),
            vec![TokenKind::Truncated(Truncation::PushBeyondEnd {
                at: 0,
                declared: 0x20,
                remaining: 2,
            })]
        );

        // A PUSHDATA2 whose own length field is cut off.
        let script = asm("raw:4d01");
        assert_eq!(
            kinds(&script),
            vec![TokenKind::Truncated(Truncation::LengthFieldCutOff {
                at: 0,
                needed: 2,
                remaining: 1,
            })]
        );

        // Truncation is terminal: nothing is yielded after it.
        let script = asm("OP_DUP raw:4effffffff OP_DUP");
        let kinds = kinds(&script);
        assert_eq!(kinds.len(), 2, "{kinds:?}");
        assert!(matches!(kinds[1], TokenKind::Truncated(_)));
    }

    #[test]
    fn parse10_empty_script_yields_no_tokens() {
        assert_eq!(kinds(&[]), Vec::new());
    }

    // Unbalanced branches are a fact about the script, not a parse failure.
    #[test]
    fn parse11_unbalanced_branches_do_not_panic_or_underflow() {
        let stray_endif = asm("OP_ENDIF OP_ENDIF OP_1");
        let depths: Vec<u32> = tokenise(&stray_endif).map(|t| t.depth).collect();
        assert_eq!(depths, vec![0, 0, 0], "depth must not wrap below zero");

        let unclosed = asm("OP_IF OP_IF OP_1");
        let depths: Vec<u32> = tokenise(&unclosed).map(|t| t.depth).collect();
        assert_eq!(depths, vec![0, 1, 2]);

        let stray_else = asm("OP_ELSE");
        assert_eq!(kinds(&stray_else), vec![TokenKind::Branch(BranchOp::Else)]);
    }

    // The criterion carried over from CS-404: the token stream must be
    // lossless. Round-tripping reconstructs from (encoding, element) only —
    // no token retains a copy of its input bytes — so an encoding the
    // tokeniser collapsed would fail to come back.
    #[test]
    fn parse12_every_fixture_round_trips_to_the_exact_input() {
        let key = "02111111111111111111111111111111111111111111111111111111111111111e";
        let h160 = "aabbccddeeff00112233445566778899aabbccdd";

        let sources = vec![
            "OP_DUP OP_HASH160 0xaabbccddeeff00112233445566778899aabbccdd OP_EQUALVERIFY OP_CHECKSIG".to_string(),
            "OP_1NEGATE".to_string(),
            "0x81".to_string(),
            "OP_PUSHDATA1 0x81".to_string(),
            "OP_PUSHDATA2 0x81".to_string(),
            "OP_PUSHDATA4 0x81".to_string(),
            format!("OP_PUSHDATA1 0x{key}"),
            format!("OP_PUSHDATA2 0x{key}"),
            format!("OP_PUSHDATA4 0x{key}"),
            "OP_0 OP_1 OP_16 OP_1NEGATE".to_string(),
            "OP_IF OP_RETURN OP_ENDIF 0xaabb OP_CHECKSIG".to_string(),
            format!("OP_RETURN 0x76a914{h160}88ac"),
            "OP_RETURN".to_string(),
            "OP_PUSHDATA1 0x".to_string(),
            "OP_PUSHDATA4 0x".to_string(),
            "raw:ff".to_string(),
            "raw:006a".to_string(),
        ];

        for src in sources {
            let script = asm(&src);
            assert_eq!(
                reassemble(&script),
                script,
                "{src} must round-trip; tokens were {:?}",
                kinds(&script)
            );
        }
    }

    // Non-minimal encodings are the ones a lossy path collapses silently, so
    // they get their own assertion rather than relying on the list above.
    #[test]
    fn parse13_non_minimal_encodings_survive_the_round_trip() {
        let minimal = asm("0x81");
        let pushdata1 = asm("OP_PUSHDATA1 0x81");

        assert_ne!(minimal, pushdata1);
        assert_eq!(reassemble(&minimal), minimal);
        assert_eq!(reassemble(&pushdata1), pushdata1);

        // Same element, different encoding — which is precisely what must not
        // be collapsed.
        assert_eq!(elements(&minimal), elements(&pushdata1));
        assert_ne!(reassemble(&minimal), reassemble(&pushdata1));
    }

    // Deep nesting is a counter, so this costs one u32 rather than a stack
    // frame per level. A recursive parser would abort here, and a stack
    // overflow in Rust cannot be caught.
    #[test]
    fn parse14_deep_nesting_does_not_grow_the_stack() {
        const DEPTH: usize = 100_000;
        let script = asm(&format!(
            "{} OP_1 {}",
            "OP_IF ".repeat(DEPTH),
            "OP_ENDIF ".repeat(DEPTH)
        ));

        let mut max_depth = 0;
        let mut count = 0usize;
        for token in tokenise(&script) {
            max_depth = max_depth.max(token.depth);
            count += 1;
        }
        assert_eq!(count, DEPTH * 2 + 1);
        assert_eq!(max_depth, DEPTH as u32);
    }

    // Walking a large script must not materialise it. This asserts the shape
    // of the guarantee — the iterator is driven to completion without
    // collecting — rather than measuring allocation, which a unit test cannot
    // do portably.
    #[test]
    fn parse15_a_large_script_walks_without_collecting() {
        let payload = "5a".repeat(1024 * 1024);
        let script = asm(&format!("OP_PUSHDATA4 0x{payload} OP_CHECKSIG"));

        let mut pushes = 0usize;
        let mut total_element_bytes = 0usize;
        for token in tokenise(&script) {
            if let TokenKind::Push { element, .. } = token.kind {
                pushes += 1;
                total_element_bytes += element.len();
            }
        }
        assert_eq!(pushes, 1);
        assert_eq!(total_element_bytes, 1024 * 1024);
    }
}

// Throughput at the sizes a P2P payload permits. Gated behind UAAS_BENCH like
// the matcher benchmark, and only meaningful in a release build:
//   UAAS_BENCH=1 cargo test --release bench_probe -- --nocapture
#[cfg(test)]
mod bench_probe {
    use super::*;
    use std::time::Instant;

    /// A script of `size` bytes made of single-byte opcodes, which is the worst
    /// case for the tokeniser: one token per byte, no push to skip over.
    fn dense_opcodes(size: usize) -> Vec<u8> {
        // OP_DUP, OP_SWAP, OP_2DUP, OP_NOP in rotation. None is a push, so
        // nothing shortens the walk.
        const OPS: [u8; 4] = [
            op_codes::OP_DUP,
            op_codes::OP_SWAP,
            op_codes::OP_2DUP,
            op_codes::OP_NOP,
        ];
        (0..size).map(|i| OPS[i % OPS.len()]).collect()
    }

    /// The opposite shape: one enormous push. The walk is two tokens however
    /// large it gets, because the element is borrowed rather than copied.
    fn one_large_push(size: usize) -> Vec<u8> {
        let mut script = Vec::with_capacity(size + 5);
        script.push(op_codes::OP_PUSHDATA4);
        script.extend_from_slice(&(size as u32).to_le_bytes());
        script.resize(size + 5, 0x5a);
        script
    }

    #[test]
    fn bench_probe_tokeniser_throughput() {
        if std::env::var("UAAS_BENCH").is_err() {
            eprintln!("skipping bench_probe: UAAS_BENCH not set");
            return;
        }
        if cfg!(debug_assertions) {
            eprintln!("bench_probe: debug build, numbers are not meaningful");
        }

        println!(
            "\n{:>8} | {:>16} | {:>12} | {:>12} | {:>10}",
            "size", "shape", "tokens", "elapsed", "MB/s"
        );
        println!(
            "{:->8}-+-{:->16}-+-{:->12}-+-{:->12}-+-{:->10}",
            "", "", "", "", ""
        );

        for mb in [1usize, 8, 32] {
            let size = mb * 1024 * 1024;
            for (shape, script) in [
                ("dense opcodes", dense_opcodes(size)),
                ("one large push", one_large_push(size)),
            ] {
                let start = Instant::now();
                let tokens = tokenise(&script).count();
                let elapsed = start.elapsed().as_secs_f64();

                // Below a microsecond the clock says nothing useful, and the
                // one-large-push shape lands there by design: it is two tokens
                // whatever its size, because the element is borrowed.
                let rate = if elapsed > 1e-6 {
                    format!("{:.0}", (size as f64 / (1024.0 * 1024.0)) / elapsed)
                } else {
                    "n/a".to_string()
                };
                println!(
                    "{:>5} MB | {:>16} | {:>12} | {:>9.3} ms | {rate:>10}",
                    mb,
                    shape,
                    tokens,
                    elapsed * 1000.0,
                );
            }
        }
        println!();
    }
}
