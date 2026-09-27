//! Loading a reviewed chainstate export into `utxo` and `utxo_monitor`.
//!
//! The logic lives here rather than in the binary so it can be tested: a
//! `src/bin` target is a crate of its own and nothing can link it.
//!
//! # What this is not
//!
//! It is not a second matcher. The C++ scanner's filter is deliberately coarse
//! and over-inclusive; its job ends at cutting hundreds of millions of records
//! down to a reviewable set. The authority is [`crate::uaas::selection`], which
//! the live indexer also calls, and a candidate it rejects is not loaded.
//!
//! # What it refuses to do
//!
//! **Overwrite anything.** The live path writes `ON CONFLICT DO UPDATE`,
//! because it is the authority on an outpoint it has just seen. A backfill is
//! not: if a row is already there, the service put it there from the chain and
//! knows more than a snapshot does. Every statement here is `DO NOTHING`, which
//! is also what makes a second run over the same export an exact no-op rather
//! than a rewrite that happens to produce the same bytes.
//!
//! **Resurrect a spent output.** Chainstate says an output was unspent when the
//! snapshot was taken. `utxo_spent` says the service later saw it spent. The
//! service's view is the newer one, so a candidate already in `utxo_spent` is
//! skipped. Without this, a load would put spent coins back into the spendable
//! set and nothing would ever take them out again.
//!
//! # Isolation
//!
//! Statements assume they are the only writer. The insert is guarded on the
//! outpoint being absent from `utxo_spent`, and under PostgreSQL's default
//! READ COMMITTED that guard is evaluated against a snapshot taken when the
//! statement begins — so a spend committed by a concurrent service between the
//! check and the insert would not be seen. That race is not closed by a
//! stricter isolation level here; it is closed by [`assert_no_other_sessions`],
//! which refuses to run at all while anything else is attached to the database.

use anyhow::{bail, Context, Result};
use postgres::Client;

use crate::candidate_export::{Candidate, Export};
use crate::uaas::collection::WorkingCollection;
use crate::uaas::selection::{self, Selection};

/// What one run did.
///
/// Every candidate is accounted for in exactly one of these: the totals must
/// add up to the number of rows read, which is asserted in the tests. A
/// candidate quietly falling through would be a false negative, and a false
/// negative in a backfill cannot be recovered without another full scan.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LoadTotals {
    /// Rows read from the export.
    pub read: u64,
    /// Rows the real matcher accepted and that became new `utxo` rows.
    pub inserted: u64,
    /// Accepted, but the outpoint was already in `utxo`. Left alone.
    pub already_present: u64,
    /// Accepted, but the outpoint is in `utxo_spent`: the service has seen it
    /// spent since the snapshot was taken.
    pub already_spent: u64,
    /// Rejected by the real matcher. Expected and healthy — the coarse filter
    /// is allowed to be over-inclusive.
    pub not_matched: u64,
    /// Provably unspendable (`OP_RETURN`), so not part of the UTXO set.
    pub unspendable: u64,
    /// `script_is_raw = false`: the node stored a compressed template, so the
    /// bytes are a hash rather than a script and cannot be matched.
    pub compressed_template: u64,
    /// Rows in `utxo_monitor` written.
    pub monitor_rows: u64,
    /// The highest candidate height seen, for the staleness check.
    pub max_height: i32,
}

impl LoadTotals {
    /// Every row read must land in exactly one bucket.
    pub fn accounted_for(&self) -> u64 {
        self.inserted
            + self.already_present
            + self.already_spent
            + self.not_matched
            + self.unspendable
            + self.compressed_template
    }
}

/// Whether to write or only to report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Write,
    /// Decide everything, write nothing. The totals are what a real run would
    /// do, except that `inserted` counts what *would* be inserted.
    DryRun,
}

/// Refuse to run while anything else is attached to this database.
///
/// `Utxo` holds the live set in a `HashMap` filled once at startup, so a row
/// inserted underneath a running service is invisible to it until restart.
/// Worse, `Utxo::settle` does nothing at all for an outpoint that is not in
/// that map — so a spend of a freshly-inserted row would be dropped silently
/// and the row would stay spendable for ever.
///
/// Every other session is refused, not just ones that look like the service.
/// A session started before `application_name` was set carries none, so
/// matching on the name would pass exactly the case that matters. The cost is
/// that an idle `psql` blocks a load, which is a message an operator can act
/// on; the alternative is silent corruption.
pub fn assert_no_other_sessions(client: &mut Client) -> Result<()> {
    let rows = client
        .query(
            "SELECT pid, \
                    coalesce(application_name, ''), \
                    coalesce(host(client_addr), 'local'), \
                    coalesce(state, '') \
               FROM pg_stat_activity \
              WHERE datname = current_database() \
                AND pid <> pg_backend_pid() \
              ORDER BY pid",
            &[],
        )
        .context("reading pg_stat_activity to check for other sessions")?;

    if rows.is_empty() {
        return Ok(());
    }

    let mut described = String::new();
    for row in &rows {
        let pid: i32 = row.get(0);
        let application: String = row.get(1);
        let address: String = row.get(2);
        let state: String = row.get(3);
        described.push_str(&format!(
            "\n  pid {pid} from {address} ({}) state {state}",
            if application.is_empty() {
                "no application_name"
            } else {
                &application
            }
        ));
    }

    bail!(
        "{} other session(s) are attached to this database:{described}\n\n\
         Stop them and run this again. The service caches the whole spendable set in \
         memory at startup, so rows inserted underneath it are invisible until it \
         restarts -- and a spend of one of those rows is dropped without a trace \
         rather than reported.",
        rows.len()
    );
}

/// Warn when the export is a long way behind the chain the service has indexed.
///
/// The ticket asks for this against the export's tip height. The manifest
/// records a tip *hash* rather than a height -- the chainstate `B` record holds
/// `hashBestChain` -- and resolving a hash to a height needs the block index,
/// which is a different database and not part of an export. The highest
/// candidate height is available and answers the same question: a set whose
/// newest output is far below the chain tip is a stale snapshot.
///
/// A warning rather than a refusal. A deliberately old snapshot is a legitimate
/// thing to load; one loaded by accident is not, and the difference is not
/// visible from here.
pub fn warn_if_stale(client: &mut Client, export_max_height: i32, tolerance: i32) -> Result<()> {
    let row = client
        .query_opt("SELECT max(height) FROM blocks", &[])
        .context("reading the indexed chain height")?;
    let Some(indexed) = row.and_then(|r| r.get::<_, Option<i32>>(0)) else {
        log::warn!(
            "the blocks table is empty, so the export's freshness cannot be checked; \
             its newest output is at height {export_max_height}"
        );
        return Ok(());
    };
    let behind = indexed.saturating_sub(export_max_height);
    if behind > tolerance {
        log::warn!(
            "the export is stale: its newest output is at height {export_max_height} and \
             the service has indexed to {indexed}, {behind} blocks further on. Outputs \
             created since the snapshot are not in it, and outputs it holds may already \
             have been spent."
        );
    }
    Ok(())
}

/// The decision for one candidate, before any database work.
///
/// Separated from the writing so it can be tested without a database, which is
/// most of what is worth testing: the matcher's verdict, the template case and
/// the unspendable case are all pure.
pub enum Verdict {
    Load(Selection),
    NotMatched,
    Unspendable,
    CompressedTemplate,
}

pub fn decide(collections: &[WorkingCollection], candidate: &Candidate, script: &[u8]) -> Verdict {
    // A compressed template is a 20- or 32-byte hash, not a script. Running a
    // locking-script pattern over it would be meaningless, and could match by
    // coincidence -- which is worse than not matching, because the result looks
    // like a real row.
    if !candidate.script_is_raw {
        return Verdict::CompressedTemplate;
    }
    if !selection::is_spendable(script) {
        return Verdict::Unspendable;
    }
    match selection::select(collections, script) {
        Some(selection) => Verdict::Load(selection),
        None => Verdict::NotMatched,
    }
}

/// Load an opened, verified export.
///
/// The caller is responsible for having checked the schema version, the
/// absence of other sessions, and the export's digests. Those are refusals
/// about whether to run at all, and doing them here would mean this function
/// could not be tested without reproducing all of them.
pub fn load(
    client: &mut Client,
    export: &mut Export,
    collections: &[WorkingCollection],
    mode: Mode,
    progress_every: u64,
) -> Result<LoadTotals> {
    let mut totals = LoadTotals::default();
    let candidates: Vec<Candidate> = export
        .candidates()?
        .collect::<Result<Vec<_>>>()
        .context("reading the candidate file")?;

    for candidate in &candidates {
        totals.read += 1;
        totals.max_height = totals.max_height.max(candidate.height);

        let script = export.script(candidate)?;
        match decide(collections, candidate, &script) {
            Verdict::CompressedTemplate => {
                totals.compressed_template += 1;
                continue;
            }
            Verdict::Unspendable => {
                totals.unspendable += 1;
                continue;
            }
            Verdict::NotMatched => {
                totals.not_matched += 1;
                continue;
            }
            Verdict::Load(selection) => {
                let outcome = write_one(client, candidate, &script, &selection, mode)?;
                match outcome {
                    Written::Inserted(monitor_rows) => {
                        totals.inserted += 1;
                        totals.monitor_rows += monitor_rows;
                    }
                    Written::AlreadyPresent => totals.already_present += 1,
                    Written::AlreadySpent => totals.already_spent += 1,
                }
            }
        }

        if progress_every > 0 && totals.read % progress_every == 0 {
            // warn!, not info!: `release_max_level_warn` compiles info out of a
            // release build, so an operator watching a released binary would
            // see nothing at all during a load that takes hours.
            log::warn!(
                "loaded {} of {} candidates ({} inserted, {} already present, {} not matched)",
                totals.read,
                candidates.len(),
                totals.inserted,
                totals.already_present,
                totals.not_matched
            );
        }
    }

    Ok(totals)
}

enum Written {
    Inserted(u64),
    AlreadyPresent,
    AlreadySpent,
}

fn write_one(
    client: &mut Client,
    candidate: &Candidate,
    script: &[u8],
    selection: &Selection,
    mode: Mode,
) -> Result<Written> {
    let txid = &candidate.txid[..];
    let vout = i32::try_from(candidate.vout)
        .with_context(|| format!("vout {} does not fit a signed column", candidate.vout))?;

    // Asked first and separately from the insert, so the reason a candidate was
    // skipped can be reported. A single guarded INSERT would conflate "already
    // spendable" with "already spent", and those mean different things to
    // whoever reads the totals.
    let spent = client
        .query_opt(
            "SELECT 1 FROM utxo_spent WHERE txid = $1 AND vout = $2",
            &[&txid, &vout],
        )
        .context("checking utxo_spent")?;
    if spent.is_some() {
        return Ok(Written::AlreadySpent);
    }

    if mode == Mode::DryRun {
        let present = client
            .query_opt(
                "SELECT 1 FROM utxo WHERE txid = $1 AND vout = $2",
                &[&txid, &vout],
            )
            .context("checking utxo")?;
        return Ok(if present.is_some() {
            Written::AlreadyPresent
        } else {
            Written::Inserted(selection.monitors.len() as u64)
        });
    }

    // DO NOTHING, not DO UPDATE. The live path updates because it is the
    // authority on an outpoint it just saw; a backfill is not, and a row that
    // is already there came from the chain rather than from a snapshot.
    let inserted = client
        .execute(
            "INSERT INTO utxo (txid, vout, satoshis, locking_script, identifier, created_height) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (txid, vout) DO NOTHING",
            &[
                &txid,
                &vout,
                &candidate.satoshis,
                &script,
                &selection.identifier,
                &candidate.height,
            ],
        )
        .context("inserting into utxo")?;

    if inserted == 0 {
        return Ok(Written::AlreadyPresent);
    }

    let mut monitor_rows = 0;
    for monitor in &selection.monitors {
        monitor_rows += client
            .execute(
                "INSERT INTO utxo_monitor (txid, vout, monitor) VALUES ($1, $2, $3) \
                 ON CONFLICT (txid, vout, monitor) DO NOTHING",
                &[&txid, &vout, monitor],
            )
            .context("inserting into utxo_monitor")?;
    }

    Ok(Written::Inserted(monitor_rows))
}
