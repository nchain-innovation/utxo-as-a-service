use std::time::Instant;

use crate::db::PooledConn;

use crate::{
    config::{CollectionConfig, Config},
    uaas::hex_pattern::ScriptMatcher,
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

    pub fn load_txs(&mut self, collection_name: &str) -> Vec<Hash256> {
        // load txs- tx hash from database
        let start = Instant::now();
        let txs: Vec<Vec<u8>> = match self.conn.query(
            "SELECT hash FROM collection WHERE monitor = $1",
            &[&collection_name],
        ) {
            Ok(rows) => rows.iter().map(|row| row.get(0)).collect(),
            Err(err) => {
                log::error!("Unable to load collection txs for {collection_name}: {err:?}");
                return Vec::new();
            }
        };

        let retval: Vec<Hash256> = txs
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
    pub txs: Vec<Hash256>,
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
                txs: Vec::new(),
                locking_script_regex: Some(locking_script_regex),
            });
        }

        if let Some(ref pattern) = collection.locking_script_pattern {
            let locking_script_regex = ScriptMatcher::compile(pattern)?;

            return Ok(WorkingCollection {
                collection: collection.clone(),
                txs: Vec::new(),
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
        };

        WorkingCollection {
            // this is a collection that also maintains a list of tx hashes that it has used
            collection: broadcast_collection,
            txs: Vec::new(),
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
        self.txs.iter().any(|x| x == &hash)
    }

    pub fn match_any_locking_script(&self, tx: &Tx) -> bool {
        if let Some(locking_script_regex) = &self.locking_script_regex {
            for vout in &tx.outputs {
                // Match the script bytes directly. Encoding to hex here cost
                // more than the match itself and allowed nibble-misaligned
                // matches; the pattern was translated to bytes at compile time.
                if locking_script_regex.is_match(&vout.lock_script.0) {
                    return true;
                }
            }
        }
        false
    }

    pub fn push(&mut self, hash: Hash256) {
        // Add to our list of known txs
        self.txs.push(hash);
    }

    pub fn is_decendant(&self, tx: &Tx) -> bool {
        // Return true if transaction is a decendant of a known `collection` transaction.
        for vin in &tx.inputs {
            if self.txs.iter().any(|x| x == &vin.prev_output.hash) {
                return true;
            }
        }
        false
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
            },
            Network::BSV_Testnet,
        );
        assert!(result.is_err(), "an odd-length literal must be rejected");
    }
}
