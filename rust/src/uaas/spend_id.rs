//! A spend's identity, independent of how it was encoded.
//!
//! A transaction's txid covers the whole transaction, unlocking scripts
//! included, so two announcements that spend the same outputs and pay the same
//! outputs have different txids if their unlocking scripts differ by a single
//! no-op. That is transaction malleability, and it is why the txid cannot be
//! used to decide whether two announcements are the same spend.
//!
//! The provisional identity here covers only what makes a spend what it is
//! economically: **what it consumes and what it pays**. Two announcements with
//! the same identity are the same spend under two encodings. Two that consume
//! the same outpoint but have *different* identities are a genuine
//! double-spend, and that is a different event with a different severity.
//!
//! "Provisional" because it is what the service has to work with until a block
//! decides which encoding is the real one.

use chain_gang::messages::{OutPoint, Tx};
use chain_gang::util::sha256::sha256;

/// 32 bytes identifying a spend by its prevouts and its outputs.
pub type SpendId = [u8; 32];

/// Computes the provisional identity of `tx`.
///
/// **Inputs are sorted; outputs are not.** Reordering a transaction's inputs
/// does not change what it consumes or pays, so two announcements that differ
/// only in input order are the same spend. Reordering *outputs* does change
/// the transaction — an outpoint is a txid and an index, so moving an output
/// moves the coin — and such a pair is correctly treated as two different
/// spends of the same prevouts, which is a double-spend.
///
/// Lengths are folded in before the variable-length parts so that no two
/// distinct transactions can produce the same byte stream by concatenation:
/// without them, one long script and two short ones could line up.
pub fn provisional_id(tx: &Tx) -> SpendId {
    let mut prevouts: Vec<&OutPoint> = tx.inputs.iter().map(|vin| &vin.prev_output).collect();
    // `Hash256` orders by its raw bytes, which is arbitrary but stable, and
    // stability is the only property needed here.
    prevouts.sort_unstable_by(|a, b| a.hash.0.cmp(&b.hash.0).then(a.index.cmp(&b.index)));

    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&(prevouts.len() as u64).to_le_bytes());
    for prevout in prevouts {
        buf.extend_from_slice(&prevout.hash.0);
        buf.extend_from_slice(&prevout.index.to_le_bytes());
    }

    buf.extend_from_slice(&(tx.outputs.len() as u64).to_le_bytes());
    for vout in &tx.outputs {
        buf.extend_from_slice(&vout.satoshis.to_le_bytes());
        buf.extend_from_slice(&(vout.lock_script.0.len() as u64).to_le_bytes());
        buf.extend_from_slice(&vout.lock_script.0);
    }

    // sha256 here returns a Vec; the length is fixed by the algorithm, so the
    // conversion cannot fail and an expect would be dead code either way.
    let digest = sha256(&buf);
    let mut id = [0u8; 32];
    id.copy_from_slice(&digest[..32]);
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use chain_gang::messages::{OutPoint, TxIn, TxOut};
    use chain_gang::script::Script;
    use chain_gang::util::Hash256;

    fn outpoint(marker: u8, index: u32) -> OutPoint {
        OutPoint {
            hash: Hash256([marker; 32]),
            index,
        }
    }

    fn tx_with(inputs: Vec<OutPoint>, outputs: Vec<(i64, Vec<u8>)>, unlock: &[u8]) -> Tx {
        Tx {
            version: 2,
            inputs: inputs
                .into_iter()
                .map(|prev_output| TxIn {
                    prev_output,
                    unlock_script: Script(unlock.to_vec()),
                    ..TxIn::default()
                })
                .collect(),
            outputs: outputs
                .into_iter()
                .map(|(satoshis, script)| TxOut {
                    satoshis,
                    lock_script: Script(script),
                })
                .collect(),
            lock_time: 0,
        }
    }

    #[test]
    fn sid01_malleating_the_unlocking_script_does_not_change_the_identity() {
        let a = tx_with(vec![outpoint(0x11, 0)], vec![(900, vec![0xaa])], &[0x51]);
        let b = tx_with(
            vec![outpoint(0x11, 0)],
            vec![(900, vec![0xaa])],
            &[0x51, 0x61], // OP_1 OP_NOP: behaviourally identical
        );

        assert_ne!(a.hash(), b.hash(), "the fixture must be a malleated pair");
        assert_eq!(
            provisional_id(&a),
            provisional_id(&b),
            "same prevouts and same outputs is the same spend"
        );
    }

    #[test]
    fn sid02_paying_a_different_amount_is_a_different_spend() {
        let a = tx_with(vec![outpoint(0x11, 0)], vec![(900, vec![0xaa])], &[0x51]);
        let b = tx_with(vec![outpoint(0x11, 0)], vec![(800, vec![0xaa])], &[0x51]);
        assert_ne!(provisional_id(&a), provisional_id(&b));
    }

    #[test]
    fn sid03_paying_a_different_script_is_a_different_spend() {
        let a = tx_with(vec![outpoint(0x11, 0)], vec![(900, vec![0xaa])], &[0x51]);
        let b = tx_with(vec![outpoint(0x11, 0)], vec![(900, vec![0xbb])], &[0x51]);
        assert_ne!(provisional_id(&a), provisional_id(&b));
    }

    #[test]
    fn sid04_consuming_a_different_outpoint_is_a_different_spend() {
        let a = tx_with(vec![outpoint(0x11, 0)], vec![(900, vec![0xaa])], &[0x51]);
        let b = tx_with(vec![outpoint(0x11, 1)], vec![(900, vec![0xaa])], &[0x51]);
        assert_ne!(
            provisional_id(&a),
            provisional_id(&b),
            "the index is part of the outpoint"
        );
    }

    #[test]
    fn sid05_input_order_does_not_matter_but_output_order_does() {
        let one = outpoint(0x11, 0);
        let two = outpoint(0x22, 0);
        let outputs = vec![(900, vec![0xaa]), (100, vec![0xbb])];

        let forwards = tx_with(vec![one.clone(), two.clone()], outputs.clone(), &[0x51]);
        let backwards = tx_with(vec![two, one], outputs.clone(), &[0x51]);
        assert_eq!(
            provisional_id(&forwards),
            provisional_id(&backwards),
            "reordering inputs changes neither what is consumed nor what is paid"
        );

        let swapped = tx_with(
            vec![outpoint(0x11, 0), outpoint(0x22, 0)],
            vec![outputs[1].clone(), outputs[0].clone()],
            &[0x51],
        );
        assert_ne!(
            provisional_id(&forwards),
            provisional_id(&swapped),
            "reordering outputs moves the coins, so it is a different spend"
        );
    }

    /// The script length prefixes earn their place, and this fixture is
    /// constructed so that it fails without them rather than merely looking
    /// as though it would.
    ///
    /// The first draft of this test used two obviously different output
    /// lists and passed with the prefixes removed, because the fixed-width
    /// `satoshis` field happened to separate the scripts. These two encode to
    /// **the same bytes** when the prefixes are dropped:
    ///
    /// ```text
    /// a: [2] [1][aa bb] [1][cc]
    /// b: [2] [1][aa]    [443][00 cc]
    ///
    /// both -> 0200..00 0100..00 aabb 0100..00 cc
    /// ```
    ///
    /// 443 is `0xbb 0x01 0x00 …` little-endian, which is the tail of the
    /// first script followed by the low bytes of the second amount. Total
    /// script length matches too, which is what makes the streams line up
    /// exactly.
    #[test]
    fn sid06_concatenation_cannot_forge_a_collision() {
        let a = tx_with(
            vec![outpoint(0x11, 0)],
            vec![(1, vec![0xaa, 0xbb]), (1, vec![0xcc])],
            &[0x51],
        );
        let b = tx_with(
            vec![outpoint(0x11, 0)],
            vec![(1, vec![0xaa]), (443, vec![0x00, 0xcc])],
            &[0x51],
        );
        assert_ne!(
            provisional_id(&a),
            provisional_id(&b),
            "two different spends must not share an identity"
        );
    }

    #[test]
    fn sid07_the_identity_is_stable_across_calls() {
        let a = tx_with(vec![outpoint(0x11, 0)], vec![(900, vec![0xaa])], &[0x51]);
        assert_eq!(provisional_id(&a), provisional_id(&a));
    }
}
