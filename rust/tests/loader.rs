//! Loading a reviewed export.
//!
//! Split deliberately. The decision — does the real matcher accept this
//! candidate, and what does it yield — is pure and tested here unconditionally.
//! The writing needs PostgreSQL and is skipped, not failed, without
//! `UAAS_TEST_POSTGRES_URL`, following the convention in `src/migrate.rs`.

use chain_gang::messages::Tx;
use chain_gang::network::Network;
use chain_gang::util::Serializable;

use uaas::candidate_export::{txid_from_display, Candidate, Export};
use uaas::config::CollectionConfig;
use uaas::loader::{decide, LoadTotals, Verdict};
use uaas::uaas::collection::WorkingCollection;

/// Testnet block 1,173,457's coinbase, exactly as it sits on disk.
///
/// Real chain bytes, not a fixture: this is the transaction whose position was
/// proved against the block index in CS-447, and its single output is an
/// ordinary P2PKH paying 156,250,000 satoshis. Using it means the txid this
/// test checks is one that exists.
const REAL_COINBASE: &str = "\
0100000001000000000000000000000000000000000000000000000000000000000000\
0000ffffffff4403d1e71100fe8adbcc59fe4dd601000963676d696e6572343208\
0c000000000000002074726164653a50726574747950656e6e792d3e426f6e6e79\
426974636f696e2100ffffffff01902f5009000000001976a914924a8f5e24e555\
3280724dbd15e50de4ba7e3f4f88ac00000000";

fn collection(name: &str, pattern: &str) -> WorkingCollection {
    WorkingCollection::new(
        CollectionConfig {
            name: name.to_string(),
            track_descendants: false,
            address: None,
            locking_script_pattern: Some(pattern.to_string()),
        },
        Network::BSV_Testnet,
    )
    .expect("the test pattern should compile")
}

fn candidate(script_is_raw: bool, len: u64) -> Candidate {
    Candidate {
        txid_display: "aa".repeat(32),
        txid: [0xaa; 32],
        vout: 0,
        height: 700_000,
        coinbase: false,
        confiscation: false,
        satoshis: 1,
        script_type: 6 + len,
        script_is_raw,
        script_len: len,
        script_offset: 0,
        label: "coarse".to_string(),
    }
}

fn ordinal_script() -> Vec<u8> {
    // OP_FALSE OP_IF "ord", the 1SAT envelope the C++ filter selects on.
    hex::decode("0063036f7264516170706c69636174696f6e").unwrap()
}

/// The coarse filter is allowed to be over-inclusive; this loader's matcher is
/// the authority and a candidate it rejects must not be loaded. Without this
/// there would be two matchers, and the backfilled half of the table would
/// follow different rules from the live half with nothing in the data to say so.
#[test]
fn loader01_the_real_matcher_rejects_what_the_coarse_filter_let_through() {
    let collections = vec![collection("p2pkh", "^76a914[0-9a-f]{40}88ac$")];
    let script = ordinal_script();
    let row = candidate(true, script.len() as u64);

    assert!(
        matches!(decide(&collections, &row, &script), Verdict::NotMatched),
        "a candidate the configured collections do not select must not load"
    );
}

#[test]
fn loader02_a_matched_candidate_carries_its_monitors_and_identifier() {
    let collections = vec![
        collection("envelope", "^0063036f7264[0-9a-f]*$"),
        collection("captures", "^0063036f7264(?<identifier>51)[0-9a-f]*$"),
    ];
    let script = ordinal_script();
    let row = candidate(true, script.len() as u64);

    let Verdict::Load(selection) = decide(&collections, &row, &script) else {
        panic!("the candidate should load");
    };
    assert_eq!(
        selection.monitors,
        vec!["envelope".to_string(), "captures".to_string()],
        "every matching monitor, in configuration order"
    );
    assert_eq!(
        selection.identifier,
        Some(vec![0x51]),
        "the first pattern declaring an identifier supplies it"
    );
}

/// `script_is_raw = false` means the node stored a compressed template, so the
/// bytes are a 20- or 32-byte hash rather than a script. Matching a locking
/// script pattern against a hash is meaningless, and a coincidental match would
/// be worse than no match — the row would look real.
#[test]
fn loader03_a_compressed_template_is_never_matched() {
    // A pattern that matches anything at all, so the only thing that can stop
    // this loading is the template check itself.
    let collections = vec![collection("anything", "[0-9a-f]*")];
    let script = ordinal_script();
    let row = candidate(false, script.len() as u64);

    assert!(
        matches!(
            decide(&collections, &row, &script),
            Verdict::CompressedTemplate
        ),
        "a compressed template must be refused before the matcher sees it"
    );

    // The same bytes with the flag set do load, so the refusal is the flag and
    // not something about the script.
    let raw = candidate(true, script.len() as u64);
    assert!(matches!(
        decide(&collections, &raw, &script),
        Verdict::Load(_)
    ));
}

/// A provably unspendable output is not part of the UTXO set whatever its
/// script matches. Loading one would put a row in the spendable set that no
/// spend can ever settle.
#[test]
fn loader04_an_unspendable_output_is_not_loaded() {
    let collections = vec![collection("anything", "[0-9a-f]*")];
    for script_hex in ["6a03414243", "006a03414243"] {
        let script = hex::decode(script_hex).unwrap();
        let row = candidate(true, script.len() as u64);
        assert!(
            matches!(decide(&collections, &row, &script), Verdict::Unspendable),
            "{script_hex} is OP_RETURN and must not load"
        );
    }
}

/// The one conversion that fails silently when it is wrong, checked against a
/// transaction that exists.
///
/// The C++ scanner writes txids in display order, reversing what chainstate
/// stores; the `utxo` table stores internal order, which is what `Hash256`
/// holds. A missing or doubled reversal yields 32 plausible bytes that address
/// nothing, and nothing downstream would notice until a join returned no rows.
#[test]
fn loader05_a_real_txid_survives_the_display_to_internal_conversion() {
    let bytes = hex::decode(REAL_COINBASE).unwrap();
    let tx = Tx::read(&mut std::io::Cursor::new(&bytes)).expect("real chain bytes should parse");
    let hash = tx.hash();

    // What chain-gang shows a human, and what the scanner writes in the CSV.
    let display = hash.encode();
    assert_eq!(display.len(), 64);

    let recovered = txid_from_display(&display).expect("a 64-character hex txid");
    assert_eq!(
        recovered, hash.0,
        "the loader's conversion must be the exact inverse of the display encoding"
    );

    // And it is a real reversal, not an identity that happens to pass.
    let raw_hex = hex::encode(hash.0);
    assert_ne!(
        raw_hex, display,
        "this txid is deliberately not a palindrome, so a missing reversal cannot pass"
    );
}

/// The values the loader would put in a `utxo` row for a real output are the
/// values the live path would put there for the same output.
///
/// The decision itself cannot drift — both call `uaas::uaas::selection` — so
/// what is left to check is the mapping from an export row to columns. This
/// pins it against a transaction taken off the chain.
#[test]
fn loader06_a_real_output_maps_to_the_columns_the_live_path_would_write() {
    let bytes = hex::decode(REAL_COINBASE).unwrap();
    let tx = Tx::read(&mut std::io::Cursor::new(&bytes)).expect("parses");
    assert_eq!(tx.outputs.len(), 1);
    let output = &tx.outputs[0];

    // What the live path would carry for this output.
    let live_script = output.lock_script.0.clone();
    let live_satoshis = output.satoshis;
    let live_txid = tx.hash().0;

    // What an export row for the same output carries.
    let row = Candidate {
        txid_display: tx.hash().encode(),
        txid: txid_from_display(&tx.hash().encode()).unwrap(),
        vout: 0,
        height: 1_173_457,
        coinbase: true,
        confiscation: false,
        satoshis: live_satoshis,
        script_type: 6 + live_script.len() as u64,
        script_is_raw: true,
        script_len: live_script.len() as u64,
        script_offset: 0,
        label: "coarse".to_string(),
    };

    assert_eq!(row.txid, live_txid, "txid bytes");
    assert_eq!(row.satoshis, live_satoshis, "satoshis, in base units");
    assert_eq!(
        row.satoshis, 156_250_000,
        "the real coinbase value, so a change to either side shows up"
    );

    // And both sides agree it is monitored, by the same function.
    let collections = vec![collection("p2pkh", "^76a914[0-9a-f]{40}88ac$")];
    let Verdict::Load(selection) = decide(&collections, &row, &live_script) else {
        panic!("a real P2PKH output should be selected by a P2PKH pattern");
    };
    assert_eq!(selection.monitors, vec!["p2pkh".to_string()]);
}

/// Every candidate must land in exactly one bucket. One falling through would
/// be a false negative, and a false negative in a backfill cannot be found
/// again without another full chainstate scan.
#[test]
fn loader07_the_totals_account_for_every_row() {
    let totals = LoadTotals {
        read: 9,
        inserted: 2,
        already_present: 1,
        already_spent: 1,
        not_matched: 3,
        unspendable: 1,
        compressed_template: 1,
        monitor_rows: 4,
        max_height: 700_000,
    };
    assert_eq!(totals.accounted_for(), totals.read);

    // And the check is capable of failing.
    let dropped = LoadTotals { read: 10, ..totals };
    assert_ne!(dropped.accounted_for(), dropped.read);
}

/// The whole fixture export, decided end to end. Three candidates, all
/// carrying the ordinal envelope the C++ filter selected on, so a matcher
/// configured for that envelope must accept all three — and one configured for
/// P2PKH must accept only the one whose script is also P2PKH.
#[test]
fn loader08_the_fixture_export_is_decided_consistently() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/candidate_export");
    let mut export = Export::open(&dir).expect("the fixture opens");
    let rows: Vec<Candidate> = export
        .candidates()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let ordinal = vec![collection("ordinal", "0063036f7264[0-9a-f]*")];
    let mut loaded = 0;
    for row in &rows {
        let script = export.script(row).unwrap();
        if matches!(decide(&ordinal, row, &script), Verdict::Load(_)) {
            loaded += 1;
        }
    }
    assert_eq!(loaded, 3, "all three fixture candidates carry the envelope");

    // A narrower matcher takes fewer of them, which is the coarse filter being
    // over-inclusive working as intended rather than failing.
    let p2pkh = vec![collection("p2pkh", "76a914[0-9a-f]{40}88ac$")];
    let mut narrower = 0;
    for row in &rows {
        let script = export.script(row).unwrap();
        if matches!(decide(&p2pkh, row, &script), Verdict::Load(_)) {
            narrower += 1;
        }
    }
    assert_eq!(
        narrower, 1,
        "only the candidate whose script is also P2PKH; the other two are \
         correctly refused by the authoritative matcher"
    );
}

// ---------------------------------------------------------------------------
// Everything below needs PostgreSQL and is skipped without it.
// ---------------------------------------------------------------------------

mod with_database {
    use super::*;
    use postgres::Client;
    use uaas::loader::{assert_no_other_sessions, load, Mode};
    use uaas::migrate;

    /// A client on an isolated schema, or `None` to skip.
    ///
    /// One schema per test: sharing `public` means tests drop each other's
    /// tables under `cargo test`'s default parallelism, which passes serially
    /// and fails several ways in parallel. Same approach as `src/migrate.rs`.
    fn client_for(test_name: &str) -> Option<Client> {
        let Ok(url) = std::env::var("UAAS_TEST_POSTGRES_URL") else {
            eprintln!("skipping {test_name}: UAAS_TEST_POSTGRES_URL not set");
            return None;
        };
        let mut client = Client::connect(&url, postgres::NoTls).expect("connect to postgres");
        let schema: String = test_name.chars().take(48).collect();
        client
            .batch_execute(&format!(
                "DROP SCHEMA IF EXISTS {schema} CASCADE; \
                 CREATE SCHEMA {schema}; \
                 SET search_path TO {schema};"
            ))
            .expect("create an isolated schema");
        migrate::run(&mut client).expect("apply the schema");
        Some(client)
    }

    fn fixture() -> Export {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/candidate_export");
        Export::open(&dir).expect("the fixture opens")
    }

    fn ordinal_collections() -> Vec<WorkingCollection> {
        vec![collection("ordinal", "0063036f7264[0-9a-f]*")]
    }

    #[test]
    fn db01_rows_land_in_utxo_and_utxo_monitor() {
        let Some(mut client) = client_for("db01_rows_land") else {
            return;
        };
        let mut export = fixture();
        let totals = load(
            &mut client,
            &mut export,
            &ordinal_collections(),
            Mode::Write,
            0,
        )
        .expect("the load should succeed");

        assert_eq!(totals.read, 3);
        assert_eq!(totals.inserted, 3);
        assert_eq!(totals.monitor_rows, 3, "one monitor each");
        assert_eq!(totals.accounted_for(), totals.read);

        let utxo: i64 = client
            .query_one("SELECT count(*) FROM utxo", &[])
            .unwrap()
            .get(0);
        assert_eq!(utxo, 3);
        let monitor: i64 = client
            .query_one(
                "SELECT count(*) FROM utxo_monitor WHERE monitor = 'ordinal'",
                &[],
            )
            .unwrap()
            .get(0);
        assert_eq!(monitor, 3);

        // The columns, not just the count. The height is a real one, so
        // created_height must not be NULL.
        let row = client
            .query_one(
                "SELECT satoshis, created_height, locking_script FROM utxo \
                 ORDER BY created_height LIMIT 1",
                &[],
            )
            .unwrap();
        let satoshis: i64 = row.get(0);
        let height: Option<i32> = row.get(1);
        let script: Vec<u8> = row.get(2);
        assert_eq!(satoshis, 5_000_000_000);
        assert_eq!(height, Some(200));
        assert!(
            script
                .windows(6)
                .any(|w| w == hex::decode("0063036f7264").unwrap().as_slice()),
            "the stored script is the untruncated one from the sidecar"
        );
    }

    /// The natural primary keys make this available; the ticket asks for it to
    /// be tested rather than assumed.
    #[test]
    fn db02_a_second_run_over_the_same_export_inserts_nothing() {
        let Some(mut client) = client_for("db02_second_run") else {
            return;
        };
        let collections = ordinal_collections();
        let first = load(&mut client, &mut fixture(), &collections, Mode::Write, 0).unwrap();
        assert_eq!(first.inserted, 3);

        let second = load(&mut client, &mut fixture(), &collections, Mode::Write, 0).unwrap();
        assert_eq!(second.inserted, 0, "the second run must insert nothing");
        assert_eq!(second.already_present, 3);
        assert_eq!(second.accounted_for(), second.read);

        let utxo: i64 = client
            .query_one("SELECT count(*) FROM utxo", &[])
            .unwrap()
            .get(0);
        assert_eq!(utxo, 3, "and must not have duplicated anything");
    }

    /// Chainstate says the output was unspent when the snapshot was taken;
    /// `utxo_spent` says the service has seen it spent since. The service's
    /// view is newer, so the loader must not put the coin back.
    #[test]
    fn db03_an_output_the_service_has_seen_spent_is_not_resurrected() {
        let Some(mut client) = client_for("db03_not_resurrected") else {
            return;
        };
        let export = fixture();
        let first = export.candidates().unwrap().next().unwrap().unwrap();

        client
            .execute(
                "INSERT INTO utxo_spent \
                 (txid, vout, satoshis, locking_script, created_height, spent_txid, spent_height) \
                 VALUES ($1, $2, 1, '\\x00', 200, '\\xff', 700000)",
                &[&&first.txid[..], &(first.vout as i32)],
            )
            .unwrap();

        let totals = load(
            &mut client,
            &mut fixture(),
            &ordinal_collections(),
            Mode::Write,
            0,
        )
        .unwrap();
        assert_eq!(totals.already_spent, 1);
        assert_eq!(totals.inserted, 2);

        let present: i64 = client
            .query_one(
                "SELECT count(*) FROM utxo WHERE txid = $1 AND vout = $2",
                &[&&first.txid[..], &(first.vout as i32)],
            )
            .unwrap()
            .get(0);
        assert_eq!(present, 0, "a spent output must not be back in utxo");
    }

    /// The live path writes `ON CONFLICT DO UPDATE` because it is the authority
    /// on an outpoint it has just seen. A backfill is not: a row already there
    /// came from the chain and knows more than a snapshot does.
    #[test]
    fn db04_an_existing_row_is_left_alone() {
        let Some(mut client) = client_for("db04_left_alone") else {
            return;
        };
        let export = fixture();
        let first = export.candidates().unwrap().next().unwrap().unwrap();

        client
            .execute(
                "INSERT INTO utxo (txid, vout, satoshis, locking_script, created_height) \
                 VALUES ($1, $2, 999, '\\xdeadbeef', 12345)",
                &[&&first.txid[..], &(first.vout as i32)],
            )
            .unwrap();

        let totals = load(
            &mut client,
            &mut fixture(),
            &ordinal_collections(),
            Mode::Write,
            0,
        )
        .unwrap();
        assert_eq!(totals.already_present, 1);

        let row = client
            .query_one(
                "SELECT satoshis, locking_script, created_height FROM utxo \
                 WHERE txid = $1 AND vout = $2",
                &[&&first.txid[..], &(first.vout as i32)],
            )
            .unwrap();
        let satoshis: i64 = row.get(0);
        let script: Vec<u8> = row.get(1);
        let height: Option<i32> = row.get(2);
        assert_eq!(satoshis, 999, "the existing row was overwritten");
        assert_eq!(script, hex::decode("deadbeef").unwrap());
        assert_eq!(height, Some(12345));
    }

    /// A dry run decides everything and writes nothing.
    #[test]
    fn db05_a_dry_run_writes_nothing() {
        let Some(mut client) = client_for("db05_dry_run") else {
            return;
        };
        let totals = load(
            &mut client,
            &mut fixture(),
            &ordinal_collections(),
            Mode::DryRun,
            0,
        )
        .unwrap();
        assert_eq!(totals.inserted, 3, "it reports what it would insert");

        let utxo: i64 = client
            .query_one("SELECT count(*) FROM utxo", &[])
            .unwrap()
            .get(0);
        assert_eq!(utxo, 0, "and writes none of them");
    }

    /// The guard that stops a load corrupting a running service.
    ///
    /// Only the positive direction is checked. The clean case needs a database
    /// nothing else is attached to, which a test suite sharing one cannot
    /// arrange — and asserting it here would make the test pass or fail on how
    /// many other tests happened to be running.
    #[test]
    fn db06_another_session_is_detected() {
        let Some(mut client) = client_for("db06_other_session") else {
            return;
        };
        let url = std::env::var("UAAS_TEST_POSTGRES_URL").unwrap();
        let _other = Client::connect(&url, postgres::NoTls).expect("a second session");

        let err =
            assert_no_other_sessions(&mut client).expect_err("a second session must be detected");
        let message = format!("{err}");
        assert!(
            message.contains("other session"),
            "unhelpful message: {message}"
        );
        assert!(
            message.contains("restarts") || message.contains("invisible"),
            "the message should say why it matters: {message}"
        );
    }
}
