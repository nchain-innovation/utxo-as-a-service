//! Does a pattern match describe executable script feeding a signature check?
//!
//! The permissive property a collection establishes by default — "these bytes
//! appear somewhere in this locking script" — is satisfiable by anyone for the
//! cost of a dust output. `MatchProperty::SignatureOperand` asks the question
//! consumers were already assuming the answer to: *is the thing this pattern
//! selected an operand to a signature check on a path that executes?*
//!
//! # Two rules, not one
//!
//! CS-415 describes only the second. The probe corpus needs both.
//!
//! 1. **Token alignment.** The match must cover whole opcodes, or exactly one
//!    push element. A pattern that matches *inside* a push element has found
//!    data, not script — which is what `OP_RETURN <the template>` and
//!    `OP_IF OP_RETURN OP_ENDIF <the template>` both are. This is CS-402's
//!    fix one level up: that stopped nibble-misaligned matches, this stops
//!    element-interior ones.
//! 2. **Signature reachability.** The element the match selects must reach an
//!    `OP_CHECKSIG`, directly or through a hash equality that binds it to a
//!    key that is checked. A key pushed and then `OP_DROP`ped is perfectly
//!    aligned and still proves nothing.
//!
//! # Conservative, deliberately
//!
//! This is static analysis, not execution. Anything it cannot model poisons
//! the walk and the answer becomes "cannot prove", never "probably yes": a
//! false positive here is the exact failure the strict property exists to
//! remove. Branches poison for the same reason — whether a branch executes
//! depends on the unlocking script, which an output does not carry.
//!
//! The locking script is analysed alone, so the values the unlocking script
//! would supply are modelled as opaque and materialised on demand when the
//! script pops from an empty stack.

use std::collections::HashSet;
use std::ops::Range;

use super::script_parse::{tokenise, Token, TokenKind};

// Opcodes this analysis models. Anything absent poisons the walk.
const OP_DUP: u8 = 0x76;
const OP_DROP: u8 = 0x75;
const OP_2DROP: u8 = 0x6d;
const OP_NIP: u8 = 0x77;
const OP_SWAP: u8 = 0x7c;
const OP_VERIFY: u8 = 0x69;
const OP_EQUAL: u8 = 0x87;
const OP_EQUALVERIFY: u8 = 0x88;
const OP_RIPEMD160: u8 = 0xa6;
const OP_SHA1: u8 = 0xa7;
const OP_SHA256: u8 = 0xa8;
const OP_HASH160: u8 = 0xa9;
const OP_HASH256: u8 = 0xaa;
const OP_CHECKSIG: u8 = 0xac;
const OP_CHECKSIGVERIFY: u8 = 0xad;

/// A value on the abstract stack.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Node {
    /// An element this script pushed, identified by its token index.
    Pushed(usize),
    /// Supplied by the unlocking script, or otherwise unnamed.
    Opaque,
    /// A hash of another node.
    Hash(usize),
}

/// `None` means the walk lost track of this slot.
type Slot = Option<usize>;

struct Machine {
    arena: Vec<Node>,
    stack: Vec<Slot>,
    /// Nodes consumed as the key operand of a signature check.
    checked: HashSet<usize>,
    /// `(pushed, inner)` — a pushed element was compared equal to a hash of
    /// `inner`, so checking `inner` establishes the pushed element too. This
    /// is the P2PKH shape: the script commits to `HASH160(key)`, and the key
    /// itself is what `OP_CHECKSIG` verifies.
    bindings: Vec<(usize, usize)>,
    poisoned: bool,
}

impl Machine {
    fn new() -> Self {
        Machine {
            arena: Vec::new(),
            stack: Vec::new(),
            checked: HashSet::new(),
            bindings: Vec::new(),
            poisoned: false,
        }
    }

    fn alloc(&mut self, node: Node) -> usize {
        self.arena.push(node);
        self.arena.len() - 1
    }

    /// Pop, materialising an opaque value when the script reaches past its own
    /// pushes into what the unlocking script supplied.
    fn pop(&mut self) -> Slot {
        match self.stack.pop() {
            Some(slot) => slot,
            None => Some(self.alloc(Node::Opaque)),
        }
    }

    fn push(&mut self, slot: Slot) {
        self.stack.push(slot);
    }
}

/// Byte ranges of each token, parallel to `tokens`.
fn spans(tokens: &[Token], len: usize) -> Vec<Range<usize>> {
    (0..tokens.len())
        .map(|i| {
            let start = tokens[i].offset;
            let end = tokens.get(i + 1).map_or(len, |next| next.offset);
            start..end
        })
        .collect()
}

/// Which tokens a match covers, or `None` if it is not aligned to any.
///
/// Accepts a run of whole tokens, or exactly one push element. The second case
/// is what makes the answer independent of push encoding: the same 33-byte key
/// under `OP_PUSHDATA1` and under a minimal push is the same element, and a
/// pattern written against either should reach the same verdict.
fn aligned_tokens(
    tokens: &[Token],
    spans: &[Range<usize>],
    script_len: usize,
    range: &Range<usize>,
) -> Option<Vec<usize>> {
    // Whole tokens.
    let first = spans.iter().position(|s| s.start == range.start);
    if let Some(first) = first {
        if let Some(last) = spans.iter().rposition(|s| s.end == range.end) {
            if last >= first {
                let covered: Vec<usize> = (first..=last).collect();
                // Data after a top-level OP_RETURN is not script, and a
                // truncated tail was never parsed.
                if covered.iter().all(|&i| {
                    !matches!(
                        tokens[i].kind,
                        TokenKind::TrailingData(_) | TokenKind::Truncated(_)
                    )
                }) {
                    return Some(covered);
                }
            }
        }
    }

    // Exactly one push element.
    for (i, token) in tokens.iter().enumerate() {
        if let TokenKind::Push { element, .. } = token.kind {
            let element_end = spans[i].end.min(script_len);
            let element_start = element_end.saturating_sub(element.len());
            if range.start == element_start && range.end == element_end {
                return Some(vec![i]);
            }
        }
    }
    None
}

/// Whether any element among `covered` reaches a signature check.
fn reaches_signature_check(tokens: &[Token], covered: &[usize]) -> bool {
    let mut m = Machine::new();

    for (i, token) in tokens.iter().enumerate() {
        match token.kind {
            TokenKind::Push { .. } => {
                let id = m.alloc(Node::Pushed(i));
                m.push(Some(id));
            }
            // Whether a branch executes depends on the unlocking script, which
            // an output does not carry. Nothing after this can be proven.
            TokenKind::Branch(_) => {
                m.poisoned = true;
                break;
            }
            // At depth 0 this ends the script; the tokeniser only yields it at
            // depth 0 as a terminator, and deeper it sits inside a branch,
            // which has already poisoned the walk.
            TokenKind::Return => break,
            TokenKind::TrailingData(_) | TokenKind::Truncated(_) => break,
            TokenKind::Op(op) => {
                if !step(&mut m, op) {
                    m.poisoned = true;
                    break;
                }
            }
        }
    }

    if m.poisoned {
        return false;
    }

    covered.iter().any(|&token_index| {
        m.arena
            .iter()
            .enumerate()
            .filter(|(_, node)| **node == Node::Pushed(token_index))
            .any(|(id, _)| {
                m.checked.contains(&id)
                    || m.bindings
                        .iter()
                        .any(|(pushed, inner)| *pushed == id && m.checked.contains(inner))
            })
    })
}

/// One opcode. Returns false when the opcode is not modelled.
fn step(m: &mut Machine, op: u8) -> bool {
    match op {
        // Numeric and data pushes that the tokeniser reports as opcodes
        // (OP_0, OP_1..OP_16, OP_1NEGATE) put a value on the stack whose
        // identity does not matter here.
        0x00 | 0x4f | 0x51..=0x60 => {
            m.push(None);
            true
        }
        OP_DUP => {
            let top = m.pop();
            m.push(top);
            m.push(top);
            true
        }
        OP_DROP => {
            m.pop();
            true
        }
        OP_2DROP => {
            m.pop();
            m.pop();
            true
        }
        OP_NIP => {
            let top = m.pop();
            m.pop();
            m.push(top);
            true
        }
        OP_SWAP => {
            let a = m.pop();
            let b = m.pop();
            m.push(a);
            m.push(b);
            true
        }
        OP_VERIFY => {
            m.pop();
            true
        }
        OP_RIPEMD160 | OP_SHA1 | OP_SHA256 | OP_HASH160 | OP_HASH256 => {
            let inner = m.pop();
            let slot = inner.map(|id| m.alloc(Node::Hash(id)));
            m.push(slot);
            true
        }
        OP_EQUAL | OP_EQUALVERIFY => {
            let a = m.pop();
            let b = m.pop();
            bind(m, a, b);
            bind(m, b, a);
            if op == OP_EQUAL {
                m.push(None);
            }
            true
        }
        OP_CHECKSIG | OP_CHECKSIGVERIFY => {
            // Key on top, signature beneath.
            let key = m.pop();
            m.pop();
            if let Some(id) = key {
                m.checked.insert(id);
            }
            if op == OP_CHECKSIG {
                m.push(None);
            }
            true
        }
        // OP_CHECKMULTISIG and everything else: not modelled. The key count is
        // a stack value this analysis does not evaluate, so the operands
        // cannot be identified without executing the script.
        _ => false,
    }
}

/// Record that a pushed element was compared equal to a hash of something.
fn bind(m: &mut Machine, pushed: Slot, other: Slot) {
    let (Some(pushed), Some(other)) = (pushed, other) else {
        return;
    };
    if !matches!(m.arena.get(pushed), Some(Node::Pushed(_))) {
        return;
    }
    if let Some(Node::Hash(inner)) = m.arena.get(other).copied() {
        m.bindings.push((pushed, inner));
    }
}

/// Whether the match at `range` establishes the strict property.
pub fn is_signature_operand(script: &[u8], range: Range<usize>) -> bool {
    if range.start >= range.end || range.end > script.len() {
        return false;
    }
    let tokens: Vec<Token> = tokenise(script).collect();
    let spans = spans(&tokens, script.len());
    let Some(covered) = aligned_tokens(&tokens, &spans, script.len(), &range) else {
        return false;
    };
    reaches_signature_check(&tokens, &covered)
}
