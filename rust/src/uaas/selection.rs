//! What a monitor selects, decided in one place.
//!
//! The live indexer and the backfill loader both have to answer the same
//! question: given a locking script, is this output monitored, by which
//! monitors, and what identifier does it carry? If they answer it separately
//! they will drift, and a drifted backfill is worse than none — the historical
//! and live halves of one table would follow different rules with nothing in
//! the data to say which row followed which.
//!
//! So the answer lives here and both callers use it. The parity test in
//! `uaas-load-utxo` asserts that a row the loader writes matches the row the
//! live path would have written; this module is what makes that true by
//! construction rather than by coincidence.
//!
//! Note what is *not* here: the loader does not reimplement the matcher, and
//! the C++ scanner's filter is not this. That filter is deliberately coarse and
//! over-inclusive, and its job ends at cutting volume. This is the authority.

use super::collection::WorkingCollection;

/// Everything the tables need to know about a monitored output, beyond the
/// output itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Every monitor whose pattern selected this script, in configuration
    /// order. Never empty: an empty selection is `None`, not a `Selection`.
    pub monitors: Vec<String>,
    /// The bytes the first matching pattern captured, if any declared an
    /// identifier group.
    pub identifier: Option<Vec<u8>>,
}

/// Whether an output can ever be spent.
///
/// A provably unspendable output is not part of the UTXO set whatever its
/// script says, so recording one would inflate the set with rows no spend can
/// ever settle.
///
/// Takes the script rather than a `TxOut` because the loader has no
/// transaction — it reads a script and a value out of a chainstate export —
/// and the two paths have to test the same thing.
///
/// Previously the equivalent check named `0x61` (`OP_NOP`) in its comment while
/// testing `0x6a`. The code was right and the comment was not (CS-436).
pub fn is_spendable(script: &[u8]) -> bool {
    const OP_FALSE: u8 = 0x00;
    const OP_RETURN: u8 = 0x6a;

    !(script.starts_with(&[OP_RETURN]) || script.starts_with(&[OP_FALSE, OP_RETURN]))
}

/// Which monitors select `script`, and what identifier it carries.
///
/// `None` when nothing selects it, which is the overwhelmingly common case and
/// is not an error.
///
/// Order matters twice: it decides which pattern's capture becomes the
/// identifier when several match, and it is what makes that choice
/// reproducible. `collections` is built from the configuration in order, so the
/// same script always yields the same identifier.
pub fn select(collections: &[WorkingCollection], script: &[u8]) -> Option<Selection> {
    // Decided once. `matches_script` honours the collection's required
    // property; `identifier_in` does not, and only reads the pattern. Taking
    // the identifier from `collections` rather than from the ones that selected
    // would let a collection that rejected the script put its capture on a row
    // attributed to someone else (CS-477).
    let selecting: Vec<&WorkingCollection> = collections
        .iter()
        .filter(|c| c.matches_script(script))
        .collect();
    if selecting.is_empty() {
        return None;
    }
    let monitors: Vec<String> = selecting.iter().map(|c| c.name().to_string()).collect();
    let identifier = selecting
        .iter()
        .find_map(|c| c.identifier_in(script))
        .map(<[u8]>::to_vec);
    Some(Selection {
        monitors,
        identifier,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CollectionConfig, MatchProperty};
    use chain_gang::network::Network;

    /// Most of these tests are about which monitors a pattern selects and which
    /// capture becomes the identifier, not about CS-415's structural property.
    /// Stated rather than defaulted so a change to the default cannot quietly
    /// change what they cover.
    fn collection(name: &str, pattern: &str) -> WorkingCollection {
        collection_requiring(name, pattern, MatchProperty::BytesPresent)
    }

    fn collection_requiring(
        name: &str,
        pattern: &str,
        require: MatchProperty,
    ) -> WorkingCollection {
        WorkingCollection::new(
            CollectionConfig {
                name: name.to_string(),
                track_descendants: false,
                address: None,
                locking_script_pattern: Some(pattern.to_string()),
                require,
            },
            Network::BSV_Testnet,
        )
        .expect("test pattern should compile")
    }

    /// P2PKH: OP_DUP OP_HASH160 <20 bytes> OP_EQUALVERIFY OP_CHECKSIG.
    fn p2pkh(hash160: u8) -> Vec<u8> {
        let mut script = vec![0x76, 0xa9, 0x14];
        script.extend(std::iter::repeat_n(hash160, 20));
        script.extend([0x88, 0xac]);
        script
    }

    #[test]
    fn select01_an_unmatched_script_selects_nothing() {
        let collections = vec![collection("p2pkh", "76a914[0-9a-f]{40}88ac")];
        assert_eq!(select(&collections, &[0x51, 0x52, 0x53]), None);
    }

    #[test]
    fn select02_a_matched_script_names_its_monitor() {
        let collections = vec![collection("p2pkh", "76a914[0-9a-f]{40}88ac")];
        let found = select(&collections, &p2pkh(0x42)).expect("should match");
        assert_eq!(found.monitors, vec!["p2pkh".to_string()]);
        assert_eq!(found.identifier, None, "no identifier group was declared");
    }

    /// An output can belong to several monitors at once, which is why
    /// `utxo_monitor` is its own table rather than a column.
    #[test]
    fn select03_every_matching_monitor_is_named_in_configuration_order() {
        let collections = vec![
            collection("first", "76a914[0-9a-f]{40}88ac"),
            collection("second", "^76a9[0-9a-f]+$"),
            collection("nomatch", "^6a[0-9a-f]*$"),
        ];
        let found = select(&collections, &p2pkh(0x42)).expect("should match");
        assert_eq!(
            found.monitors,
            vec!["first".to_string(), "second".to_string()],
            "matching monitors, in configuration order, and only those"
        );
    }

    /// The identifier comes from the *first* matching pattern that declares
    /// one. Reproducibility is the point: the live path and the loader must
    /// pick the same bytes for the same script, every time.
    #[test]
    fn select04_the_first_declaring_pattern_supplies_the_identifier() {
        let collections = vec![
            collection("plain", "76a914[0-9a-f]{40}88ac"),
            collection("captures", "76a914(?<identifier>[0-9a-f]{40})88ac"),
        ];
        let found = select(&collections, &p2pkh(0x42)).expect("should match");
        assert_eq!(found.monitors.len(), 2);
        assert_eq!(
            found.identifier,
            Some(vec![0x42u8; 20]),
            "the second pattern declares the group, so its capture is used"
        );
    }

    #[test]
    fn select05_an_earlier_capture_wins_over_a_later_one() {
        let collections = vec![
            collection("early", "76a914(?<identifier>[0-9a-f]{40})88ac"),
            collection("late", "(?<identifier>76a914[0-9a-f]{40}88ac)"),
        ];
        let found = select(&collections, &p2pkh(0x42)).expect("should match");
        assert_eq!(
            found.identifier,
            Some(vec![0x42u8; 20]),
            "configuration order decides, not pattern length or specificity"
        );
    }

    /// `OP_RETURN` followed by one push whose payload is the bytes of a P2PKH
    /// script. Data, not script: a match inside a push element is what
    /// `SignatureOperand` rejects, and the push is well formed (0x19 = 25, the
    /// length of the template) so that is the only reason it can be rejected.
    fn p2pkh_bytes_as_op_return_data() -> Vec<u8> {
        let mut script = vec![0x6a, 0x19];
        script.extend(p2pkh(0x42));
        script
    }

    /// CS-477. Which monitors selected a script and what identifier it carries
    /// are one decision. A collection that rejected the script under its
    /// required property must not contribute a capture to the row, or the row
    /// is attributed to one monitor while carrying another monitor's bytes.
    #[test]
    fn select06_a_collection_that_rejected_the_script_supplies_no_identifier() {
        let strict_pattern = "76a914(?<identifier>[0-9a-f]{40})88ac";
        let strict =
            || collection_requiring("strict", strict_pattern, MatchProperty::SignatureOperand);

        // Control: the same collection does select a genuine P2PKH and does
        // yield its capture, so the rejection below is the property at work and
        // not a pattern that never matched.
        let genuine = select(&[strict()], &p2pkh(0x42)).expect("strict selects a real P2PKH");
        assert_eq!(genuine.monitors, vec!["strict".to_string()]);
        assert_eq!(genuine.identifier, Some(vec![0x42u8; 20]));

        let data = p2pkh_bytes_as_op_return_data();
        assert_eq!(
            select(&[strict()], &data),
            None,
            "the template inside a push is data, so strict selects nothing"
        );

        let collections = vec![strict(), collection("loose", "^6a[0-9a-f]*$")];
        let found = select(&collections, &data).expect("loose selects it");
        assert_eq!(found.monitors, vec!["loose".to_string()]);
        assert_eq!(
            found.identifier, None,
            "strict rejected the script, so its capture is not on the row"
        );
    }

    /// CS-477, the other direction: a rejecting collection earlier in
    /// configuration order must not pre-empt the capture of one that selected.
    #[test]
    fn select07_a_rejecting_collection_does_not_preempt_a_selecting_one() {
        let collections = vec![
            collection_requiring(
                "strict",
                "76a914(?<identifier>[0-9a-f]{40})88ac",
                MatchProperty::SignatureOperand,
            ),
            collection("loose", "6a19(?<identifier>[0-9a-f]{4})"),
        ];
        let found = select(&collections, &p2pkh_bytes_as_op_return_data()).expect("loose selects");
        assert_eq!(found.monitors, vec!["loose".to_string()]);
        assert_eq!(
            found.identifier,
            Some(vec![0x76, 0xa9]),
            "the capture is loose's own two bytes, not strict's twenty"
        );
    }

    /// Both forms of provably unspendable output. Recording either would put a
    /// row in the spendable set that no spend can ever settle.
    #[test]
    fn spendable01_op_return_outputs_are_not_spendable() {
        assert!(!is_spendable(&[0x6a]), "bare OP_RETURN");
        assert!(!is_spendable(&[0x6a, 0x01, 0xff]), "OP_RETURN with data");
        assert!(!is_spendable(&[0x00, 0x6a]), "OP_FALSE OP_RETURN");
        assert!(
            !is_spendable(&[0x00, 0x6a, 0x03, 0x6f, 0x72, 0x64]),
            "OP_FALSE OP_RETURN with data"
        );
    }

    #[test]
    fn spendable02_ordinary_scripts_are_spendable() {
        assert!(is_spendable(&p2pkh(0x42)));
        // OP_FALSE OP_IF, the 1SAT inscription envelope: starts 00 63, not
        // 00 6a, so it is spendable and must not be confused with the other.
        assert!(is_spendable(&[0x00, 0x63, 0x03, 0x6f, 0x72, 0x64]));
        assert!(is_spendable(&[]), "an empty script is not OP_RETURN");
    }
}
