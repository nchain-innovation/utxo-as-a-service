//! Load a reviewed chainstate export into `utxo` and `utxo_monitor`.
//!
//! The last step of the chainstate backfill. `bitcoin_cdbwrapper` reads a
//! node's chainstate and writes an export; a human reviews it; this loads what
//! was approved.
//!
//! The two halves are joined by a file and by nothing else, which is what makes
//! the review step structural rather than procedural — two programs joined by a
//! file cannot skip it by accident. The corollary is that this side has to
//! treat the file as a contract: it refuses an export it cannot identify, one
//! whose digests do not match, and one whose hash is not the hash that was
//! approved.
//!
//! Everything it does is described in `docs/Backfill.md`.

use std::path::PathBuf;
use std::process;

use anyhow::{bail, Context, Result};
use chain_gang::network::Network;
use postgres::{Client, NoTls};

use uaas::{
    candidate_export::Export,
    config::{CollectionConfig, DatabaseConfig},
    db, loader, migrate,
    uaas::collection::WorkingCollection,
};

const USAGE: &str = "\
usage:
  uaas-load-utxo --export DIR --approve SHA256 [options]

  --export DIR      a reviewed export directory from bitcoin_cdbwrapper
  --approve SHA256  the SHA-256 of that export's export.toml, as approved.
                    The manifest records the digests of the other two files,
                    so this one value commits to the whole export.
  --config FILE     TOML holding [database] and [collection].
                    Defaults to ../data/uaasr.toml -- the service's own
                    configuration, which is the point: the collections have to
                    be the ones the service uses, or a backfilled row follows
                    different rules from a live one.
  --dry-run         decide everything, write nothing, report the totals.
  --progress-every N  log progress every N candidates (default 10000, 0 off).
  --stale-tolerance N  blocks the export may lag the indexed chain before it is
                    called stale (default 1000).

The service must not be running. This refuses to start while anything else is
attached to the database, and the service needs restarting afterwards: it
caches the spendable set at startup and will not see these rows until it does.";

/// Only the two sections this tool needs.
///
/// Deserialised from any TOML that has them, so it can be pointed at the
/// service's own configuration file and inherit exactly the collections the
/// service uses. Unknown sections -- peers, start heights, the web interface --
/// are ignored rather than rejected.
#[derive(Debug, serde::Deserialize)]
struct LoaderConfig {
    database: DatabaseConfig,
    #[serde(default)]
    collection: Vec<CollectionConfig>,
    #[serde(default)]
    service: Option<ServiceSection>,
}

#[derive(Debug, serde::Deserialize)]
struct ServiceSection {
    network: String,
}

struct Options {
    export: PathBuf,
    approve: String,
    config: PathBuf,
    dry_run: bool,
    progress_every: u64,
    stale_tolerance: i32,
}

fn parse_args() -> Result<Option<Options>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut export = None;
    let mut approve = None;
    let mut config = PathBuf::from("../data/uaasr.toml");
    let mut dry_run = false;
    let mut progress_every = 10_000u64;
    let mut stale_tolerance = 1000i32;

    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let mut value = |what: &str| -> Result<String> {
            index += 1;
            args.get(index)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("{what} needs a value"))
        };
        match arg {
            "--export" => export = Some(PathBuf::from(value("--export")?)),
            "--approve" => approve = Some(value("--approve")?),
            "--config" => config = PathBuf::from(value("--config")?),
            "--dry-run" => dry_run = true,
            "--progress-every" => progress_every = value("--progress-every")?.parse()?,
            "--stale-tolerance" => stale_tolerance = value("--stale-tolerance")?.parse()?,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            other => bail!("unknown argument {other:?}\n\n{USAGE}"),
        }
        index += 1;
    }

    let (Some(export), Some(approve)) = (export, approve) else {
        bail!("--export and --approve are both required\n\n{USAGE}");
    };
    Ok(Some(Options {
        export,
        approve,
        config,
        dry_run,
        progress_every,
        stale_tolerance,
    }))
}

// Not `#[tokio::main]`, for the reason `db::build_pool` documents: the
// synchronous postgres client drives a runtime of its own and panics if it is
// called from inside one. This tool never needs a runtime at all.
fn main() {
    simple_logger::init_with_level(log::Level::Warn).expect("logger");
    if let Err(err) = run() {
        // {err:#} so anyhow's context chain is printed rather than only the
        // outermost message.
        eprintln!("uaas-load-utxo: {err:#}");
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let Some(options) = parse_args()? else {
        return Ok(());
    };

    let text = std::fs::read_to_string(&options.config)
        .with_context(|| format!("cannot read {}", options.config.display()))?;
    let config: LoaderConfig =
        toml::from_str(&text).with_context(|| format!("parsing {}", options.config.display()))?;

    let network = match config.service.as_ref().map(|s| s.network.as_str()) {
        Some("mainnet") => Network::BSV_Mainnet,
        Some("stn") => Network::BSV_STN,
        // Testnet by default. An address-based collection compiles to a
        // different locking script per network, so this is not cosmetic -- but
        // defaulting to mainnet would be the wrong way to be wrong given the
        // standing restriction on mainnet work.
        _ => Network::BSV_Testnet,
    };

    if config.collection.is_empty() {
        bail!(
            "{} declares no [[collection]]. With no collections nothing matches and the \
             load would be an expensive no-op; point --config at the service's own \
             configuration",
            options.config.display()
        );
    }
    let collections: Vec<WorkingCollection> = config
        .collection
        .iter()
        .map(|c| WorkingCollection::new(c.clone(), network))
        .collect::<Result<Vec<_>, _>>()
        .context("compiling the collection patterns")?;
    log::warn!(
        "{} collection(s) will decide what loads: {}",
        collections.len(),
        collections
            .iter()
            .map(|c| c.name().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );

    // Opened before the database is touched: an export that cannot be
    // identified is a reason to stop before connecting to anything.
    let mut export = Export::open(&options.export)
        .with_context(|| format!("opening {}", options.export.display()))?;

    let actual = export.manifest_sha256()?;
    if actual != options.approve.trim().to_lowercase() {
        bail!(
            "this export's manifest hashes to {actual}, not the {} that was approved. \
             Either it is not the export that was reviewed, or it changed after review",
            options.approve
        );
    }
    log::warn!("manifest {actual} matches the approved hash");

    export
        .verify_digests()
        .context("the export does not match the digests in its own manifest")?;
    log::warn!(
        "digests verified: {} candidates, true as of block {}",
        export.manifest().candidates,
        export.manifest().tip_block_hash
    );

    // One connection, opened directly, and deliberately not through
    // `db::build_pool`. r2d2 fills a pool to its maximum size as soon as it is
    // built, so a pooled loader opens ten connections and then refuses to run
    // because it can see nine of them in pg_stat_activity -- it would block
    // itself. A one-shot single-threaded tool has no use for a pool anyway.
    let mut conn = Client::connect(&config.database.postgres_url, NoTls).with_context(|| {
        format!(
            "could not connect to {}",
            db::redact_url(&config.database.postgres_url)
        )
    })?;

    migrate::assert_expected_version(&mut conn)?;
    loader::assert_no_other_sessions(&mut conn)?;

    let mode = if options.dry_run {
        log::warn!("dry run: nothing will be written");
        loader::Mode::DryRun
    } else {
        loader::Mode::Write
    };

    let totals = loader::load(
        &mut conn,
        &mut export,
        &collections,
        mode,
        options.progress_every,
    )?;

    loader::warn_if_stale(&mut conn, totals.max_height, options.stale_tolerance)?;

    // Every total at warn: `release_max_level_warn` compiles info! out of a
    // release build, so this is the only level an operator running a released
    // binary would see.
    log::warn!(
        "{}: {} candidates read, {} inserted, {} monitor rows, {} already present, \
         {} already spent, {} not matched by the real matcher, {} unspendable, \
         {} compressed templates",
        if options.dry_run { "dry run" } else { "loaded" },
        totals.read,
        totals.inserted,
        totals.monitor_rows,
        totals.already_present,
        totals.already_spent,
        totals.not_matched,
        totals.unspendable,
        totals.compressed_template,
    );

    // The totals must account for every row. A candidate falling through
    // silently would be a false negative, and a false negative in a backfill
    // cannot be found again without another full chainstate scan.
    if totals.accounted_for() != totals.read {
        bail!(
            "internal error: {} candidates read but {} accounted for",
            totals.read,
            totals.accounted_for()
        );
    }

    if !options.dry_run && totals.inserted > 0 {
        log::warn!(
            "restart the service. It caches the spendable set at startup, so it cannot \
             see these {} rows until it does -- and a spend of a row it does not hold is \
             dropped rather than recorded",
            totals.inserted
        );
    }
    Ok(())
}
