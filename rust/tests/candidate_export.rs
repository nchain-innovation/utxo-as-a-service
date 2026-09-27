//! Reading a reviewed chainstate export.
//!
//! The fixture under `tests/fixtures/candidate_export` was written by
//! `bitcoin_cdbwrapper`'s scanner, not by hand — see the README there. A
//! hand-written fixture would agree with whatever this reader misunderstood,
//! because one would have been copied from the other.

use std::fs;
use std::path::PathBuf;

use uaas::candidate_export::{sha256_of_file, Export, CSV_VERSION_MARKER};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/candidate_export")
}

/// A writable copy, so a test that corrupts a file does not corrupt the
/// fixture every later test reads.
fn fixture_copy() -> (tempdir::Dir, PathBuf) {
    let dir = tempdir::Dir::new();
    let path = dir.path().to_path_buf();
    for name in ["candidates.csv", "scripts.dat", "export.toml"] {
        fs::copy(fixture_dir().join(name), path.join(name)).expect("copying the fixture");
    }
    (dir, path)
}

/// A temporary directory that removes itself. Small enough not to be worth a
/// dependency.
mod tempdir {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Names are made unique by a counter, not by a timestamp.
    ///
    /// The first version of this used `SystemTime::now().as_nanos()`, which is
    /// only nominally nanoseconds: macOS reports microseconds, so the value
    /// ends in three zeros. `cargo test` runs these tests in parallel threads
    /// of one process, two of them landed in the same microsecond, and they
    /// shared a directory -- one test's `Drop` then deleted the files another
    /// was still reading. That failed about one run in four, and only ever
    /// under parallelism.
    ///
    /// A counter cannot collide within a process, and the pid separates
    /// processes.
    static NEXT: AtomicU64 = AtomicU64::new(0);

    pub struct Dir(PathBuf);

    impl Dir {
        pub fn new() -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!(
                "uaas_export_test_{}_{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).expect("creating a temporary directory");
            Dir(path)
        }
        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            // Not `let _ =`: the crate warns on that on purpose. A failure to
            // clean up must not fail a test that has already passed, so it is
            // reported rather than discarded silently.
            if let Err(err) = std::fs::remove_dir_all(&self.0) {
                eprintln!("could not remove {}: {err}", self.0.display());
            }
        }
    }
}

#[test]
fn export01_the_manifest_is_read() {
    let export = Export::open(&fixture_dir()).expect("the fixture should open");
    let manifest = export.manifest();

    assert_eq!(manifest.format_version, 1);
    assert_eq!(manifest.records_scanned, 8);
    assert_eq!(manifest.records_skipped, 0);
    assert_eq!(manifest.candidates, 3);
    assert_eq!(manifest.distinct_heights, 3);
    assert_eq!(
        manifest.patterns,
        vec![("ordinal".to_string(), "0063036f7264".to_string())],
        "the coarse filter that produced the set, recorded verbatim"
    );
    // A hash, not a height. The fixture database's 'B' record is 32 bytes of
    // 0x11 rather than a real block.
    assert_eq!(manifest.tip_block_hash, "11".repeat(32));
    assert_eq!(manifest.candidates_csv.bytes, 566);
    assert_eq!(manifest.scripts.bytes, 43);
}

#[test]
fn export02_the_digests_in_the_manifest_match_the_files() {
    let export = Export::open(&fixture_dir()).expect("the fixture should open");
    export
        .verify_digests()
        .expect("the fixture should verify against its own manifest");
}

/// The check that makes review mean something. A file altered after it was
/// approved must not load.
#[test]
fn export03_a_modified_file_is_refused() {
    let (_guard, dir) = fixture_copy();
    let csv = dir.join("candidates.csv");
    let mut text = fs::read_to_string(&csv).unwrap();
    // Same length, different content: this defeats a length check alone, which
    // is why the digest is taken as well.
    text = text.replace(",5000000000,", ",5000000001,");
    assert_eq!(text.len(), fs::read_to_string(&csv).unwrap().len());
    fs::write(&csv, &text).unwrap();

    let export = Export::open(&dir).expect("it still opens");
    let err = export
        .verify_digests()
        .expect_err("a modified candidate file must be refused");
    assert!(
        format!("{err}").contains("does not match the manifest"),
        "unhelpful message: {err}"
    );
}

#[test]
fn export04_a_truncated_file_is_refused_by_length() {
    let (_guard, dir) = fixture_copy();
    let scripts = dir.join("scripts.dat");
    let bytes = fs::read(&scripts).unwrap();
    fs::write(&scripts, &bytes[..bytes.len() - 1]).unwrap();

    let export = Export::open(&dir).expect("it still opens");
    let err = export.verify_digests().expect_err("must be refused");
    let message = format!("{err}");
    assert!(
        message.contains("truncated") || message.contains("bytes"),
        "a truncated file should be named as such: {message}"
    );
}

/// An export has no manifest until its scan reaches the end of the chainstate.
/// A directory without one holds an interrupted scan — a prefix of the UTXO
/// set — and loading it would under-report while looking entirely normal.
#[test]
fn export05_an_export_with_no_manifest_is_refused() {
    let (_guard, dir) = fixture_copy();
    fs::remove_file(dir.join("export.toml")).unwrap();

    let err = match Export::open(&dir) {
        Err(err) => err,
        Ok(_) => panic!("an export with no manifest must be refused"),
    };
    assert!(
        format!("{err:#}").contains("interrupted"),
        "the message should say what a missing manifest means: {err:#}"
    );
}

#[test]
fn export06_an_unknown_format_version_is_refused() {
    let (_guard, dir) = fixture_copy();
    let manifest = dir.join("export.toml");
    let text = fs::read_to_string(&manifest)
        .unwrap()
        .replace("format_version = 1", "format_version = 2");
    fs::write(&manifest, text).unwrap();

    let err = match Export::open(&dir) {
        Err(err) => err,
        Ok(_) => panic!("a later format version must be refused"),
    };
    assert!(
        format!("{err:#}").contains("format version 2"),
        "unhelpful message: {err:#}"
    );
}

#[test]
fn export07_a_csv_without_the_version_marker_is_refused() {
    let (_guard, dir) = fixture_copy();
    let csv = dir.join("candidates.csv");
    let text = fs::read_to_string(&csv).unwrap();
    let without = text.lines().skip(1).collect::<Vec<_>>().join("\n");
    fs::write(&csv, without).unwrap();

    let export = Export::open(&dir).expect("it opens");
    let err = export
        .candidates()
        .err()
        .expect("a candidate file with no version marker must be refused");
    assert!(
        format!("{err}").contains(CSV_VERSION_MARKER),
        "the message should name what was expected: {err}"
    );
}

/// The rows themselves, including the two vout values either side of the
/// VarInt boundaries that the old C++ decoder read as zero.
#[test]
fn export08_every_row_is_read() {
    let export = Export::open(&fixture_dir()).expect("opens");
    let rows: Vec<_> = export
        .candidates()
        .expect("the candidate file should parse")
        .collect::<Result<Vec<_>, _>>()
        .expect("every row should parse");

    assert_eq!(rows.len(), 3, "three candidates, as the manifest says");

    assert_eq!(rows[0].vout, 1);
    assert_eq!(rows[0].height, 200);
    assert!(rows[0].coinbase);
    assert_eq!(rows[0].satoshis, 5_000_000_000);
    assert_eq!(rows[0].script_len, 31);
    assert_eq!(rows[0].script_offset, 0);
    assert_eq!(rows[0].label, "ordinal");

    assert_eq!(rows[1].vout, 128, "a two-byte VarInt vout");
    assert_eq!(rows[2].vout, 16384, "a three-byte VarInt vout");
    assert!(
        rows[2].confiscation,
        "the BSV confiscation flag is carried through, not folded into the height"
    );
    assert!(
        rows.iter().all(|r| r.script_is_raw),
        "every fixture script is a real script, not a compressed template"
    );
}

/// The one conversion in the loader that fails silently when it is wrong: a
/// reversed txid is 32 plausible bytes that address nothing.
#[test]
fn export09_the_txid_is_reversed_into_internal_order() {
    let export = Export::open(&fixture_dir()).expect("opens");
    let first = export
        .candidates()
        .unwrap()
        .next()
        .unwrap()
        .expect("the first row parses");

    // The CSV writes display order; the utxo table stores internal order.
    let display = hex::decode(&first.txid_display).unwrap();
    let mut expected = display.clone();
    expected.reverse();
    assert_eq!(
        first.txid.to_vec(),
        expected,
        "the stored bytes must be the display hex reversed"
    );
    assert_ne!(
        first.txid.to_vec(),
        display,
        "the fixture txid is deliberately not a palindrome, so a missing \
         reversal cannot pass this"
    );
    // Stated concretely as well, so a change to either side is visible.
    assert_eq!(
        first.txid[0], 0xa2,
        "the last display byte becomes the first stored byte"
    );
    assert_eq!(first.txid[31], 0xb2);
}

/// Offsets and lengths must cut each script out of the sidecar exactly. The
/// scripts were selected for containing the ordinal envelope, so every one of
/// them must still contain it after being read back.
#[test]
fn export10_each_script_is_read_from_its_own_offset() {
    let mut export = Export::open(&fixture_dir()).expect("opens");
    let rows: Vec<_> = export
        .candidates()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let envelope = hex::decode("0063036f7264").unwrap();
    let mut total = 0u64;
    for row in &rows {
        let script = export.script(row).expect("the script should be readable");
        assert_eq!(script.len() as u64, row.script_len);
        assert!(
            script
                .windows(envelope.len())
                .any(|window| window == envelope),
            "row {}:{} was selected for the ordinal envelope but does not contain it",
            row.txid_display,
            row.vout
        );
        // The prefix column and the sidecar are written by different code
        // paths in the scanner, so agreement between them is evidence rather
        // than restatement.
        let row_index = rows.iter().position(|r| r == row).unwrap();
        let csv = fs::read_to_string(fixture_dir().join("candidates.csv")).unwrap();
        let prefix = csv
            .lines()
            .nth(2 + row_index)
            .unwrap()
            .split(',')
            .nth(10)
            .unwrap()
            .to_string();
        assert!(
            hex::encode(&script).starts_with(&prefix),
            "the script does not begin with the prefix the CSV shows"
        );
        total += row.script_len;
    }
    assert_eq!(
        total,
        export.manifest().scripts.bytes,
        "the lengths must account for the whole script file: no gap, no overlap"
    );
}

/// Reading a script out of a sidecar that has been replaced by a shorter one
/// must fail rather than return whatever bytes happen to be there.
#[test]
fn export11_an_offset_past_the_script_file_is_an_error() {
    let (_guard, dir) = fixture_copy();
    fs::write(dir.join("scripts.dat"), b"short").unwrap();

    let mut export = Export::open(&dir).expect("opens");
    let row = export.candidates().unwrap().next().unwrap().unwrap();
    let err = export
        .script(&row)
        .expect_err("reading past the end must be an error");
    assert!(
        format!("{err:#}").contains("disagree") || format!("{err:#}").contains("fewer"),
        "unhelpful message: {err:#}"
    );
}

/// The manifest is the one value an operator approves by eye: it records the
/// digests of the other two files, so approving it commits to all three.
#[test]
fn export12_the_manifest_hash_is_the_root_of_the_chain() {
    let export = Export::open(&fixture_dir()).expect("opens");
    let root = export.manifest_sha256().expect("the manifest should hash");
    assert_eq!(root.len(), 64, "lowercase hex sha256");
    assert_eq!(
        root,
        sha256_of_file(&fixture_dir().join("export.toml")).unwrap()
    );

    // And the manifest really does commit to the other two.
    let text = fs::read_to_string(fixture_dir().join("export.toml")).unwrap();
    for name in ["candidates.csv", "scripts.dat"] {
        let digest = sha256_of_file(&fixture_dir().join(name)).unwrap();
        assert!(
            text.contains(&digest),
            "the manifest does not carry the digest of {name}"
        );
    }
}

/// A published SHA-256 vector, so the hashing is pinned to the standard rather
/// than merely being self-consistent with itself.
#[test]
fn export13_the_file_hash_matches_the_published_vector() {
    let dir = tempdir::Dir::new();
    let path = dir.path().join("abc");
    fs::write(&path, b"abc").unwrap();
    assert_eq!(
        sha256_of_file(&path).unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
