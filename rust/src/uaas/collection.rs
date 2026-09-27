use std::collections::HashSet;
use std::time::Instant;

use crate::db::PooledConn;

use crate::{
    config::{CollectionConfig, Config, MatchProperty},
    uaas::{hex_pattern::ScriptMatcher, reachability},
};
use anyhow::{anyhow, Result};
use chain_gang::{
    address::{addr_decode, AddressType},
    messages::{Payload, Tx},
    network::Network,
    transaction::p2pkh,
    util::{Hash256, Serializable},
};
use retry::{delay, retry};

/// Given an address return a locking script in hexstr format
fn address_to_lock_script(address: &str, network: Network) -> Result<String> {
    let (hash160, address_type) = addr_decode(address, network)?;
    if address_type != AddressType::P2PKH {
        return Err(anyhow!(
            "Unsupported address type for collection monitor: only P2PKH addresses are supported"
        ));
    }
    let script = p2pkh::create_lock_script(&hash160);
    Ok(hex::encode(script.0))
}

/// Database interface used by all collections
///
///
pub struct CollectionDatabase {
    // Retry database connections
    ms_delay: u64,
    retries: usize,
    conn: PooledConn,
}

impl CollectionDatabase {
    pub fn new(conn: PooledConn, config: &Config) -> Self {
        CollectionDatabase {
            ms_delay: config.database.ms_delay,
            retries: config.database.retries,
            conn,
        }
    }

    /// Stored hashes are raw `bytea` now, so this checks the length rather
    /// than parsing hex.
    fn decode_stored_hash(value: &[u8]) -> Option<Hash256> {
        match <[u8; 32]>::try_from(value) {
            Ok(bytes) => Some(Hash256(bytes)),
            Err(_) => {
                log::error!(
                    "Stored collection hash is {} bytes, expected 32; row skipped",
                    value.len()
                );
                None
            }
        }
    }

    pub fn load_txs(&mut self, collection_name: &str) -> HashSet<Hash256> {
        // load txs- tx hash from database
        let start = Instant::now();
        let txs: Vec<Vec<u8>> = match self.conn.query(
            "SELECT hash FROM collection WHERE monitor = $1",
            &[&collection_name],
        ) {
            Ok(rows) => rows.iter().map(|row| row.get(0)).collect(),
            Err(err) => {
                log::error!("Unable to load collection txs for {collection_name}: {err:?}");
                return HashSet::new();
            }
        };

        let retval: HashSet<Hash256> = txs
            .iter()
            .filter_map(|hash| Self::decode_stored_hash(hash))
            .collect();

        log::info!(
            "Collection {} Loaded {} in {} seconds",
            collection_name,
            retval.len(),
            start.elapsed().as_millis() as f64 / 1000.0
        );
        retval
    }

    pub fn write_tx_to_database(&mut self, collection_name: &str, tx: &Tx) {
        let hash256 = tx.hash();
        let hash = hash256.encode();
        // Raw bytes: the tx column is `bytea`, not hex in a `longtext`.
        let mut b = Vec::with_capacity(tx.size());
        if let Err(err) = tx.write(&mut b) {
            log::error!("Unable to serialize collection tx {hash}: {err:?}");
            return;
        }

        let result = retry(
            delay::Fixed::from_millis(self.ms_delay).take(self.retries),
            || {
                // (hash, monitor) is the primary key, and seeing the same
                // transaction twice for the same monitor is ordinary rather
                // than an error.
                self.conn.execute(
                    "INSERT INTO collection (hash, monitor, tx) VALUES ($1, $2, $3) \
                     ON CONFLICT (hash, monitor) DO NOTHING",
                    &[&&hash256.0[..], &collection_name, &b],
                )
            },
        );
        if let Err(err) = result {
            log::error!("Unable to write collection tx {hash} for {collection_name}: {err:?}");
        }
    }
}

pub struct WorkingCollection {
    // this is a collection that also maintains a list of tx hashes that it has used
    pub collection: CollectionConfig,
    /// Every transaction hash this collection holds.
    ///
    /// A set rather than a `Vec`: `have_tx` runs once per collection per
    /// transaction and `is_decendant` once per input, and both were linear
    /// scans over a list that only ever grows. Insertion order was never used
    /// — `load_txs` reads it back from the database and nothing depends on the
    /// sequence — so the set is a straight swap (CS-438).
    ///
    /// Private, so the scans cannot come back: the operations are `have_tx`,
    /// `is_decendant`, `push` and `replace_txs`.
    txs: HashSet<Hash256>,
    // No point to the Collection if there is no locking_script_regex
    // Actually there is for is_uaas_broadcast txs
    locking_script_regex: Option<ScriptMatcher>,
}

impl WorkingCollection {
    pub fn new(collection: CollectionConfig, network: Network) -> Result<Self> {
        if let Some(ref addr) = collection.address {
            // address -> locking script, in the same hex notation a pattern
            // would be written in, so both paths compile the same way.
            let pattern = address_to_lock_script(addr, network)?;
            let locking_script_regex = ScriptMatcher::compile(&pattern)?;
            return Ok(WorkingCollection {
                collection: collection.clone(),
                txs: HashSet::new(),
                locking_script_regex: Some(locking_script_regex),
            });
        }

        if let Some(ref pattern) = collection.locking_script_pattern {
            let locking_script_regex = ScriptMatcher::compile(pattern)?;

            return Ok(WorkingCollection {
                collection: collection.clone(),
                txs: HashSet::new(),
                locking_script_regex: Some(locking_script_regex),
            });
        }
        Err(anyhow!(
            "Incorrect Collection configuration {:?}",
            collection
        ))
    }

    // Create a special form of collection just to catch broadcasts
    pub fn create_broadcast_collection() -> Self {
        let broadcast_collection = CollectionConfig {
            name: "broadcast".to_string(),
            track_descendants: false,
            address: None,
            locking_script_pattern: None,
            // Carries no pattern, so nothing is ever matched against it and
            // the property is not consulted. The default keeps it honest.
            require: MatchProperty::default(),
        };

        WorkingCollection {
            // this is a collection that also maintains a list of tx hashes that it has used
            collection: broadcast_collection,
            txs: HashSet::new(),
            // No point to the Collection if there is no locking_script_regex
            // Actually there is for is_uaas_broadcast txs
            locking_script_regex: None,
        }
    }

    pub fn name(&self) -> &str {
        self.collection.name.as_str()
    }

    pub fn track_descendants(&self) -> bool {
        self.collection.track_descendants
    }

    pub fn have_tx(&self, hash: Hash256) -> bool {
        // Return true if we already have this tx hash
        self.txs.contains(&hash)
    }

    /// How many transactions this collection holds.
    ///
    /// Only the tests need this; it exists because `txs` is private and a test
    /// asserting "exactly one" should not be the reason to expose the set.
    pub fn tx_count(&self) -> usize {
        self.txs.len()
    }

    /// Replace the whole set, for the load at startup.
    ///
    /// Takes the set by value rather than exposing the field, so nothing
    /// outside can hold a reference and scan it.
    pub fn replace_txs(&mut self, txs: HashSet<Hash256>) {
        self.txs = txs;
    }

    /// Whether this collection's pattern selects a single locking script.
    ///
    /// Match the script bytes directly. Encoding to hex cost more than the
    /// match itself and allowed nibble-misaligned matches; the pattern was
    /// translated to bytes at compile time.
    /// Whether this collection selects `script`, under the property it requires.
    ///
    /// `BytesPresent` is the pattern match as it has always been. Under
    /// `SignatureOperand` the match must additionally cover whole opcodes — or
    /// exactly one push element — on an executable path, and the element it
    /// selects must reach a signature check (CS-415).
    pub fn matches_script(&self, script: &[u8]) -> bool {
        let Some(matcher) = self.locking_script_regex.as_ref() else {
            return false;
        };
        match self.collection.require {
            MatchProperty::BytesPresent => matcher.is_match(script),
            MatchProperty::SignatureOperand => matcher
                .match_range(script)
                .is_some_and(|range| reachability::is_signature_operand(script, range)),
        }
    }

    /// Which property this collection requires of a match.
    ///
    /// Exposed so a consumer can tell "these bytes are present" from "this is
    /// a signature-check operand". They are different signals and must not
    /// share a channel.
    pub fn required_property(&self) -> MatchProperty {
        self.collection.require
    }

    /// The bytes this collection's pattern captured as the output's identifier.
    ///
    /// `None` covers three different things, and the caller does not need to
    /// tell them apart: no pattern (the broadcast collection), a pattern that
    /// declares no `identifier` group, and a pattern that declares one but did
    /// not match this script.
    pub fn identifier_in<'a>(&self, script: &'a [u8]) -> Option<&'a [u8]> {
        self.locking_script_regex
            .as_ref()
            .and_then(|matcher| matcher.identifier(script))
    }

    pub fn match_any_locking_script(&self, tx: &Tx) -> bool {
        tx.outputs
            .iter()
            .any(|vout| self.matches_script(&vout.lock_script.0))
    }

    pub fn push(&mut self, hash: Hash256) {
        // Add to our list of known txs. A set, so pushing the same hash twice
        // is a no-op rather than a duplicate entry.
        self.txs.insert(hash);
    }

    pub fn is_decendant(&self, tx: &Tx) -> bool {
        // Return true if transaction is a decendant of a known `collection` transaction.
        tx.inputs
            .iter()
            .any(|vin| self.txs.contains(&vin.prev_output.hash))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chain_gang::{
        messages::{Tx, TxOut},
        network::Network,
        script::Script,
    };

    #[test]
    fn sync10_matches_locking_script_pattern() {
        let collection = CollectionConfig {
            name: "pattern".to_string(),
            track_descendants: false,
            address: None,
            locking_script_pattern: Some("76a914".to_string()),
            require: MatchProperty::BytesPresent,
        };
        let working = WorkingCollection::new(collection, Network::BSV_Testnet).expect("collection");
        let script = Script(
            hex::decode("76a9147c78584493557fac782023a4ad591b64545929d988ac").expect("script hex"),
        );
        let tx = Tx {
            version: 1,
            inputs: Vec::new(),
            outputs: vec![TxOut {
                satoshis: 1000,
                lock_script: script,
            }],
            lock_time: 0,
        };
        assert!(working.match_any_locking_script(&tx));
    }

    fn collection_for(pattern: &str) -> WorkingCollection {
        WorkingCollection::new(
            CollectionConfig {
                name: "fixture".to_string(),
                track_descendants: false,
                address: None,
                locking_script_pattern: Some(pattern.to_string()),
                require: MatchProperty::BytesPresent,
            },
            Network::BSV_Testnet,
        )
        .expect("collection compiles")
    }

    fn tx_with_script(script_hex: &str) -> Tx {
        Tx {
            version: 1,
            inputs: Vec::new(),
            outputs: vec![TxOut {
                satoshis: 1000,
                lock_script: Script(hex::decode(script_hex).expect("script hex")),
            }],
            lock_time: 0,
        }
    }

    // Every pattern configured in data/uaasr.toml, against a script it is meant
    // to select and one it is not. Matching moved from hex strings to bytes;
    // these must be unaffected.
    #[test]
    fn coll01_live_patterns_select_the_same_scripts() {
        let cases: [(&str, &str, &str, &str); 5] = [
            (
                "johns",
                "7576a914[0-9a-f]{40}88ac$",
                "7576a914111111111111111111111111111111111111111188ac",
                // same script with a trailing byte: the '$' anchor must reject it
                "7576a914111111111111111111111111111111111111111188acff",
            ),
            (
                "dsa",
                "006a[0-9a-f]{2}53417631[0-9a-f]*",
                "006a0453417631deadbeef",
                "006a0453417632deadbeef",
            ),
            (
                "CoCv1",
                "006a[0-9a-f]{2}436f437631[0-9a-f]*",
                "006a05436f43763100ff",
                "006a05436f43763200ff",
            ),
            (
                "Fin",
                "76a914c0d164cbb336e3c64338c70506ef543c2fc7b8f988ac",
                "76a914c0d164cbb336e3c64338c70506ef543c2fc7b8f988ac",
                "76a914c0d164cbb336e3c64338c70506ef543c2fc7b8f888ac",
            ),
            (
                "1sat",
                "0063036f726451126170706c69636174696f6e2f6273762d323000[0-9a-f]*",
                // the literal decodes to: OP_FALSE OP_IF "ord" OP_1 "application/bsv-20" OP_FALSE
                "0063036f726451126170706c69636174696f6e2f6273762d323000deadbeef",
                // bsv-21 rather than bsv-20
                "0063036f726451126170706c69636174696f6e2f6273762d323100deadbeef",
            ),
        ];

        for (name, pattern, should_match, should_not) in cases {
            let working = collection_for(pattern);
            assert!(
                working.match_any_locking_script(&tx_with_script(should_match)),
                "{name} should match {should_match}"
            );
            assert!(
                !working.match_any_locking_script(&tx_with_script(should_not)),
                "{name} should not match {should_not}"
            );
        }
    }

    // The correctness half of CS-402, end to end through the collection.
    #[test]
    fn coll02_nibble_misaligned_script_is_not_collected() {
        let working = collection_for("76a914[0-9a-f]{40}88ac");

        let genuine = format!("76a914{}88ac", "aa".repeat(20));
        assert!(working.match_any_locking_script(&tx_with_script(&genuine)));

        // Shifted by one nibble: the hex encoding still contains the pattern,
        // but the bytes are not a p2pkh script and contain no OP_DUP OP_HASH160.
        let shifted = format!("0{}0", genuine);
        assert!(
            !working.match_any_locking_script(&tx_with_script(&shifted)),
            "a script that only matches at an odd nibble must not be collected"
        );
    }

    // A pattern that cannot be translated faithfully must fail to build the
    // collection rather than silently monitor something else.
    #[test]
    fn coll03_untranslatable_pattern_fails_collection_construction() {
        let result = WorkingCollection::new(
            CollectionConfig {
                name: "bad".to_string(),
                track_descendants: false,
                address: None,
                locking_script_pattern: Some("76a91".to_string()),
                require: MatchProperty::BytesPresent,
            },
            Network::BSV_Testnet,
        );
        assert!(result.is_err(), "an odd-length literal must be rejected");
    }

    /// Distinct hashes, cheap to generate and spread across the key space.
    fn distinct_hashes(n: usize) -> Vec<Hash256> {
        (0..n)
            .map(|i| {
                let mut bytes = [0u8; 32];
                bytes[..8].copy_from_slice(&(i as u64).to_le_bytes());
                // Vary the high bytes too, so a hasher looking at any slice of
                // the key sees variation rather than a constant.
                bytes[24..].copy_from_slice(
                    &((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)).to_le_bytes(),
                );
                Hash256(bytes)
            })
            .collect()
    }

    /// The measurement behind CS-438: membership in a collection, `Vec` versus
    /// `HashSet`, at sizes a real collection reaches.
    ///
    /// Probes that **miss** are the case that matters. A hit can stop early,
    /// but `have_tx` returning false is the common outcome — most transactions
    /// are not already held — and that is the one that reads the whole list.
    ///
    /// `UAAS_BENCH=1 cargo test --release --lib bench_probe_collection_membership -- --nocapture`
    #[test]
    fn bench_probe_collection_membership() {
        if std::env::var("UAAS_BENCH").is_err() {
            eprintln!("skipping bench_probe: UAAS_BENCH not set");
            return;
        }
        if cfg!(debug_assertions) {
            eprintln!("bench_probe: debug build, numbers are not meaningful");
        }

        const PROBES: usize = 1000;

        println!(
            "\n{:>9} | {:>12} | {:>12} | {:>8} | {:>11} | {:>11}",
            "entries", "Vec scan", "HashSet", "ratio", "Vec bytes", "Set >="
        );
        println!(
            "{:->9}-+-{:->12}-+-{:->12}-+-{:->8}-+-{:->11}-+-{:->11}",
            "", "", "", "", "", ""
        );

        for entries in [10_000usize, 100_000, 500_000] {
            let held = distinct_hashes(entries);
            // Probes drawn from beyond the held range, so every one misses.
            let misses = distinct_hashes(entries + PROBES);
            let misses = &misses[entries..];

            let as_vec: Vec<Hash256> = held.clone();
            let as_set: HashSet<Hash256> = held.iter().copied().collect();

            let start = Instant::now();
            let mut found = 0usize;
            for h in misses {
                if as_vec.iter().any(|x| x == h) {
                    found += 1;
                }
            }
            let vec_elapsed = start.elapsed().as_secs_f64();

            let start = Instant::now();
            let mut found_set = 0usize;
            for h in misses {
                if as_set.contains(h) {
                    found_set += 1;
                }
            }
            let set_elapsed = start.elapsed().as_secs_f64();

            // Both must agree, or the comparison is measuring two different
            // questions rather than two answers to one.
            assert_eq!(found, 0, "probes were supposed to miss");
            assert_eq!(found_set, found, "the two structures disagreed");

            // Capacity is read from the structures rather than estimated.
            //
            // The set figure is a LOWER BOUND. `HashSet::capacity` reports how
            // many elements fit, not how many slots exist, and hashbrown keeps
            // spare slots above its load factor plus one control byte each. So
            // the real footprint is above this; the column is here to show the
            // order of the trade, not to be exact.
            let vec_bytes = as_vec.capacity() * std::mem::size_of::<Hash256>();
            let set_bytes = as_set.capacity() * (std::mem::size_of::<Hash256>() + 1);

            let ratio = if set_elapsed > 1e-9 {
                format!("{:.0}x", vec_elapsed / set_elapsed)
            } else {
                "n/a".to_string()
            };
            println!(
                "{entries:>9} | {:>9.3} ms | {:>9.3} ms | {ratio:>8} | {:>8} KiB | {:>8} KiB",
                vec_elapsed * 1000.0,
                set_elapsed * 1000.0,
                vec_bytes / 1024,
                set_bytes / 1024,
            );
        }
        println!();
    }
}
