//! Read a reviewed chainstate export produced by `bitcoin_cdbwrapper`.
//!
//! The format is specified in that repository's `docs/candidate_export.md`; the
//! parsing here follows it and refuses anything it does not recognise. Two
//! programs joined by a file cannot skip the human review step by accident,
//! which is the reason the boundary is a file at all — so this side has to
//! treat the file as a contract and not as a convenience.
//!
//! An export is a directory:
//!
//! ```text
//! candidates.csv   one row per candidate, bounded, what a human signs off
//! scripts.dat      the untruncated locking scripts, concatenated
//! export.toml      the manifest: version, tip, filter, counts, digests
//! ```
//!
//! # What is refused, and why
//!
//! **A directory with no `export.toml`.** The scanner writes the manifest only
//! when its pass reaches the end of the chainstate, so a scan that was stopped
//! or interrupted has none. Loading one would load a prefix of the UTXO set
//! while reporting nothing unusual.
//!
//! **A format version this build does not know.** The column set is versioned
//! as a whole: understand v1 or refuse.
//!
//! **A file whose SHA-256 does not match the manifest.** The point of the
//! review step is that what was approved is what gets loaded.
//!
//! # Streaming
//!
//! Rows are yielded one at a time and scripts are read by seeking into
//! `scripts.dat`. A testnet export measured 818 MB of scripts against a 5.9 MB
//! CSV, and a mainnet one is larger, so neither file is read whole. The digest
//! check streams for the same reason.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};

/// The only format version this build understands.
pub const SUPPORTED_FORMAT_VERSION: i64 = 1;

/// The CSV's first line, verbatim. Checked before anything else is parsed, so
/// a file of the wrong shape is refused rather than misread.
pub const CSV_VERSION_MARKER: &str = "# uaas-candidate-export v1";

const CSV_NAME: &str = "candidates.csv";
const SCRIPTS_NAME: &str = "scripts.dat";
const MANIFEST_NAME: &str = "export.toml";

/// The number of columns a v1 row has. A row with a different count is a
/// refusal, not something to pad or truncate.
const CSV_COLUMNS: usize = 12;

/// One file's entry in the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDigest {
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
}

/// What the manifest says about the export.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub format_version: i64,
    /// The chainstate directory the scan read. Provenance only.
    pub chainstate_directory: String,
    /// The block this UTXO set is true as of, display order hex.
    ///
    /// A **hash**, not a height: the chainstate `B` record holds
    /// `hashBestChain`. Turning it into a height needs the block index, which
    /// is a different database and not part of an export.
    pub tip_block_hash: String,
    pub records_scanned: u64,
    pub records_skipped: u64,
    /// The coarse filter that produced the set, as (label, hex).
    pub patterns: Vec<(String, String)>,
    pub candidates: u64,
    pub distinct_heights: u64,
    pub candidates_csv: FileDigest,
    pub scripts: FileDigest,
}

/// One row of `candidates.csv`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The txid exactly as the CSV writes it: display order, hex.
    pub txid_display: String,
    /// The same txid in internal byte order, which is what the `utxo` table
    /// stores and what `Hash256` holds.
    ///
    /// The reversal is the whole difference between the two, and getting it
    /// wrong produces 32 plausible bytes that address nothing.
    pub txid: [u8; 32],
    pub vout: u32,
    pub height: i32,
    pub coinbase: bool,
    pub confiscation: bool,
    /// Base units. Never a float, on any path.
    pub satoshis: i64,
    pub script_type: u64,
    /// False when the node stored a compressed template (`nType` 0-5), so the
    /// bytes are a 20- or 32-byte hash rather than a script.
    pub script_is_raw: bool,
    pub script_len: u64,
    pub script_offset: u64,
    /// The coarse filter's label. Advisory: this loader's own matcher decides.
    pub label: String,
}

/// A reviewed export, opened and checked.
pub struct Export {
    dir: PathBuf,
    manifest: Manifest,
    scripts: File,
}

/// Parse a manifest without opening the rest of the export.
///
/// Hand-rolled rather than run through serde: the manifest is a dozen flat
/// keys in three tables plus one array of inline tables, and a derive would
/// need a type per table plus a rename for every field. The parser below
/// rejects anything it does not recognise, which is the behaviour that matters.
fn parse_manifest(text: &str) -> Result<Manifest> {
    let mut section = String::new();
    let mut values: Vec<(String, String, String)> = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            bail!("manifest line is neither a table header nor an assignment: {line}");
        };
        values.push((
            section.clone(),
            key.trim().to_string(),
            value.trim().to_string(),
        ));
    }

    let find = |sect: &str, key: &str| -> Option<&str> {
        values
            .iter()
            .find(|(s, k, _)| s == sect && k == key)
            .map(|(_, _, v)| v.as_str())
    };
    let need = |sect: &str, key: &str| -> Result<String> {
        let raw = find(sect, key).ok_or_else(|| {
            let where_ = if sect.is_empty() {
                "at the top level".to_string()
            } else {
                format!("in [{sect}]")
            };
            anyhow!("manifest has no {key} {where_}")
        })?;
        Ok(unquote(raw))
    };
    let need_u64 = |sect: &str, key: &str| -> Result<u64> {
        need(sect, key)?
            .parse()
            .with_context(|| format!("manifest {key} is not a number"))
    };

    let format_version: i64 = need("", "format_version")?
        .parse()
        .context("manifest format_version is not a number")?;

    // Checked before any other field is read: a later version may have given
    // an existing key a new meaning, and parsing it under v1 rules would be
    // worse than refusing.
    if format_version != SUPPORTED_FORMAT_VERSION {
        bail!(
            "export is format version {format_version}; this build reads version \
             {SUPPORTED_FORMAT_VERSION}"
        );
    }

    let digest = |sect: &str| -> Result<FileDigest> {
        Ok(FileDigest {
            file: need(sect, "file")?,
            bytes: need_u64(sect, "bytes")?,
            sha256: need(sect, "sha256")?,
        })
    };

    Ok(Manifest {
        format_version,
        chainstate_directory: need("chainstate", "directory")?,
        tip_block_hash: need("chainstate", "tip_block_hash")?,
        records_scanned: need_u64("chainstate", "records_scanned")?,
        records_skipped: need_u64("chainstate", "records_skipped")?,
        patterns: parse_patterns(find("filter", "patterns").unwrap_or("[]"))?,
        candidates: need_u64("output", "candidates")?,
        distinct_heights: need_u64("output", "distinct_heights")?,
        candidates_csv: digest("output.candidates_csv")?,
        scripts: digest("output.scripts")?,
    })
}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        trimmed[1..trimmed.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    } else {
        trimmed.to_string()
    }
}

/// `[{ label = "a", hex = "00" }, { label = "b", hex = "ff" }]`.
fn parse_patterns(value: &str) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    let inner = value.trim();
    let inner = inner
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| anyhow!("manifest filter.patterns is not a list"))?;

    for part in inner.split('}') {
        let Some(body) = part.split_once('{').map(|(_, b)| b) else {
            continue;
        };
        let mut label = None;
        let mut hex = None;
        for field in body.split(',') {
            let Some((key, val)) = field.split_once('=') else {
                continue;
            };
            match key.trim() {
                "label" => label = Some(unquote(val)),
                "hex" => hex = Some(unquote(val)),
                _ => {}
            }
        }
        match (label, hex) {
            (Some(label), Some(hex)) => out.push((label, hex)),
            _ => bail!("manifest filter.patterns entry lacks a label or a hex"),
        }
    }
    Ok(out)
}

/// A txid as the CSV writes it, turned into the bytes the `utxo` table stores.
///
/// Display order is the reverse of internal order. This is the one conversion
/// in the whole loader that fails silently if it is wrong — the result is 32
/// bytes either way, and a reversed txid looks exactly as plausible as a
/// correct one until someone tries to join on it.
pub fn txid_from_display(hex_str: &str) -> Result<[u8; 32]> {
    let mut bytes = hex::decode(hex_str).with_context(|| format!("txid {hex_str:?} is not hex"))?;
    if bytes.len() != 32 {
        bail!("txid {hex_str:?} is {} bytes, not 32", bytes.len());
    }
    bytes.reverse();
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn parse_row(line: &str, line_number: usize) -> Result<Candidate> {
    let fields: Vec<&str> = line.split(',').collect();
    if fields.len() != CSV_COLUMNS {
        bail!(
            "line {line_number} has {} columns, not the {CSV_COLUMNS} a version \
             {SUPPORTED_FORMAT_VERSION} row has",
            fields.len()
        );
    }
    let at = |i: usize| fields[i];
    let boolean = |i: usize, name: &str| -> Result<bool> {
        match at(i) {
            "true" => Ok(true),
            "false" => Ok(false),
            other => bail!("line {line_number}: {name} is {other:?}, not true or false"),
        }
    };
    let number = |i: usize, name: &str| -> Result<u64> {
        at(i)
            .parse()
            .with_context(|| format!("line {line_number}: {name} is not a number"))
    };

    let height_raw = number(2, "height")?;
    // The `utxo` table's created_height is a signed integer, and -1 is the
    // sentinel for "not in a block". A chainstate entry always has a real
    // height, so anything that will not fit is a misparse rather than a
    // strange UTXO.
    let height = i32::try_from(height_raw)
        .with_context(|| format!("line {line_number}: height {height_raw} does not fit"))?;

    let satoshis_raw = number(5, "satoshis")?;
    let satoshis = i64::try_from(satoshis_raw)
        .with_context(|| format!("line {line_number}: satoshis {satoshis_raw} does not fit"))?;

    Ok(Candidate {
        txid_display: at(0).to_string(),
        txid: txid_from_display(at(0)).with_context(|| format!("line {line_number}"))?,
        vout: u32::try_from(number(1, "vout")?)
            .with_context(|| format!("line {line_number}: vout does not fit"))?,
        height,
        coinbase: boolean(3, "coinbase")?,
        confiscation: boolean(4, "confiscation")?,
        satoshis,
        script_type: number(6, "script_type")?,
        script_is_raw: boolean(7, "script_is_raw")?,
        script_len: number(8, "script_len")?,
        script_offset: number(9, "script_offset")?,
        label: at(11).to_string(),
    })
}

/// Stream SHA-256 of a file, lowercase hex.
pub fn sha256_of_file(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("cannot open {} to hash it", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("reading {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

impl Export {
    /// Open an export directory and parse its manifest.
    ///
    /// Does **not** verify the digests: that reads every byte of the export, so
    /// it is a separate call the loader makes deliberately.
    pub fn open(dir: &Path) -> Result<Self> {
        let manifest_path = dir.join(MANIFEST_NAME);
        let text = std::fs::read_to_string(&manifest_path).with_context(|| {
            format!(
                "cannot read {}. An export has no manifest until its scan reaches the end \
                 of the chainstate, so a directory without one holds an interrupted scan \
                 and must not be loaded",
                manifest_path.display()
            )
        })?;
        let manifest = parse_manifest(&text)
            .with_context(|| format!("parsing {}", manifest_path.display()))?;

        let scripts_path = dir.join(SCRIPTS_NAME);
        let scripts = File::open(&scripts_path)
            .with_context(|| format!("cannot open {}", scripts_path.display()))?;

        Ok(Export {
            dir: dir.to_path_buf(),
            manifest,
            scripts,
        })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn csv_path(&self) -> PathBuf {
        self.dir.join(CSV_NAME)
    }

    pub fn scripts_path(&self) -> PathBuf {
        self.dir.join(SCRIPTS_NAME)
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.dir.join(MANIFEST_NAME)
    }

    /// The manifest's own SHA-256.
    ///
    /// This is the one value an operator approves. The manifest records the
    /// digests of the other two files, so approving it commits to all three:
    /// one hash to check by eye, a chain to the rest.
    pub fn manifest_sha256(&self) -> Result<String> {
        sha256_of_file(&self.manifest_path())
    }

    /// Check both data files against the digests in the manifest.
    ///
    /// Length first, because a truncated file is the likely failure and saying
    /// so is more useful than "the hash differs".
    pub fn verify_digests(&self) -> Result<()> {
        for (path, expected) in [
            (self.csv_path(), &self.manifest.candidates_csv),
            (self.scripts_path(), &self.manifest.scripts),
        ] {
            let length = std::fs::metadata(&path)
                .with_context(|| format!("cannot stat {}", path.display()))?
                .len();
            if length != expected.bytes {
                bail!(
                    "{} is {length} bytes; the manifest says {}. The export is truncated \
                     or was modified after it was written",
                    path.display(),
                    expected.bytes
                );
            }
            let actual = sha256_of_file(&path)?;
            if actual != expected.sha256 {
                bail!(
                    "{} does not match the manifest: expected {}, found {actual}",
                    path.display(),
                    expected.sha256
                );
            }
        }
        Ok(())
    }

    /// Every candidate, in the order the CSV holds them.
    ///
    /// Yields a `Result` per row rather than stopping at the first bad one, so
    /// a single malformed line can be reported with its neighbours rather than
    /// aborting a load that is otherwise sound.
    pub fn candidates(&self) -> Result<impl Iterator<Item = Result<Candidate>>> {
        let path = self.csv_path();
        let file = File::open(&path).with_context(|| format!("cannot open {}", path.display()))?;
        let mut lines = BufReader::new(file).lines();

        let marker = lines
            .next()
            .transpose()?
            .ok_or_else(|| anyhow!("{} is empty", path.display()))?;
        if marker.trim() != CSV_VERSION_MARKER {
            bail!(
                "{} starts {marker:?}; this build reads exports marked {CSV_VERSION_MARKER:?}",
                path.display()
            );
        }
        // The header. Its contents are not parsed: the version marker above is
        // what pins the column set, and reading the header as authority would
        // let a file rename a column without changing its version.
        lines
            .next()
            .transpose()?
            .ok_or_else(|| anyhow!("{} has a version marker but no header", path.display()))?;

        // Line 1 is the marker and line 2 the header, so the first row is 3.
        Ok(lines.enumerate().map(|(index, line)| {
            let line = line.context("reading the candidate file")?;
            parse_row(&line, index + 3)
        }))
    }

    /// The untruncated locking script for one candidate.
    ///
    /// Seeks rather than scanning: the offsets tile `scripts.dat` in row order,
    /// so a sequential load reads the file forwards anyway without having to
    /// hold any of it.
    pub fn script(&mut self, candidate: &Candidate) -> Result<Vec<u8>> {
        let length = usize::try_from(candidate.script_len)
            .context("script length does not fit in memory")?;
        self.scripts
            .seek(SeekFrom::Start(candidate.script_offset))
            .with_context(|| {
                format!(
                    "seeking to {} for {}:{}",
                    candidate.script_offset, candidate.txid_display, candidate.vout
                )
            })?;
        let mut script = vec![0u8; length];
        self.scripts.read_exact(&mut script).with_context(|| {
            format!(
                "{} holds fewer than {length} bytes at offset {}; the export's offsets and \
                 its script file disagree",
                SCRIPTS_NAME, candidate.script_offset
            )
        })?;
        Ok(script)
    }
}
