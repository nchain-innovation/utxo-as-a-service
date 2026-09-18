//! Applies the versioned schema in `rust/migrations/`.
//!
//! # Why the schema is not created by the application any more
//!
//! Until this module, every table and index in the service was created by Rust
//! at startup — eight tables across six files, each guarded by a query against
//! `INFORMATION_SCHEMA` and then a bare `CREATE TABLE`. Three consequences, all
//! of which this replaces rather than improves:
//!
//! * the check and the create were separate statements, so two instances
//!   starting together raced;
//! * the existence probe did not filter by schema, so a same-named table
//!   anywhere visible suppressed creation;
//! * every core table had three or four definitions — production, test, CI —
//!   which had already drifted.
//!
//! The schema now lives in one place, applied once, and the service asserts the
//! version it expects rather than creating what it finds missing.
//!
//! # Guarantees
//!
//! **Each migration is atomic.** PostgreSQL's DDL is transactional, so a
//! migration that fails part-way leaves nothing behind — not a half-created
//! table, and not a row claiming it succeeded. This is the property that makes
//! a runner this small sufficient; the same design against MariaDB would need
//! a manual-repair procedure, because MySQL-family DDL commits implicitly.
//!
//! **Applied migrations are pinned by checksum.** Editing a file that has
//! already run is refused rather than ignored, because the database and the
//! repository would otherwise disagree silently.
//!
//! **A database ahead of the binary is refused.** Rolling back the service
//! without rolling back the schema is a real deployment mistake, and it should
//! fail loudly at startup rather than at the first query against a column that
//! no longer means what the code thinks.
//!
//! # Files
//!
//! `V{n}__{name}.sql`, one object per file, no `IF NOT EXISTS` — a versioned
//! migration knows whether it has run, so making the DDL idempotent would only
//! hide a runner that had lost track. They are embedded at compile time: the
//! final Docker stage carries the binary and nothing else, no `psql` and no
//! source tree.

use anyhow::{anyhow, bail, Context, Result};
use chain_gang::util::sha256::sha256;
use postgres::{Client, Transaction};

/// The schema version this build expects. A database at any other version is
/// refused rather than adapted to.
pub const EXPECTED_VERSION: i64 = 10;

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

/// The migrations, in order. Embedded rather than read from disk.
#[rustfmt::skip]
const MIGRATIONS: &[Migration] = &[
    Migration { version: 1,  name: "blocks",       sql: include_str!("../migrations/V1__blocks.sql") },
    Migration { version: 2,  name: "orphans",      sql: include_str!("../migrations/V2__orphans.sql") },
    Migration { version: 3,  name: "tx",           sql: include_str!("../migrations/V3__tx.sql") },
    Migration { version: 4,  name: "mempool",      sql: include_str!("../migrations/V4__mempool.sql") },
    Migration { version: 5,  name: "utxo",         sql: include_str!("../migrations/V5__utxo.sql") },
    Migration { version: 6,  name: "utxo_spent",   sql: include_str!("../migrations/V6__utxo_spent.sql") },
    Migration { version: 7,  name: "utxo_monitor", sql: include_str!("../migrations/V7__utxo_monitor.sql") },
    Migration { version: 8,  name: "collection",   sql: include_str!("../migrations/V8__collection.sql") },
    Migration { version: 9,  name: "addr",         sql: include_str!("../migrations/V9__addr.sql") },
    Migration { version: 10, name: "connect",      sql: include_str!("../migrations/V10__connect.sql") },
];

/// The bookkeeping table. Created outside a migration because it is what
/// records that a migration ran; `IF NOT EXISTS` is correct here and nowhere
/// else in this module.
const CREATE_BOOKKEEPING: &str = "\
CREATE TABLE IF NOT EXISTS schema_migration (
    version    bigint      PRIMARY KEY,
    name       text        NOT NULL,
    checksum   bytea       NOT NULL,
    applied_at timestamptz NOT NULL DEFAULT now()
)";

fn checksum(sql: &str) -> Vec<u8> {
    sha256(sql.as_bytes())
}

/// What a run did, so the caller can report it rather than guess.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub applied: Vec<String>,
    pub already_applied: usize,
    pub version: i64,
}

/// Applies every migration the database has not already run.
///
/// Returns an error without changing anything when the database disagrees with
/// this build about what has already been applied.
pub fn run(client: &mut Client) -> Result<Report> {
    client
        .batch_execute(CREATE_BOOKKEEPING)
        .context("creating the schema_migration table")?;

    let recorded = read_recorded(client)?;
    verify(&recorded)?;

    let mut report = Report {
        already_applied: recorded.len(),
        ..Report::default()
    };

    for migration in MIGRATIONS {
        if recorded.iter().any(|(v, _, _)| *v == migration.version) {
            continue;
        }
        apply(client, migration)
            .with_context(|| format!("applying V{}__{}", migration.version, migration.name))?;
        report
            .applied
            .push(format!("V{}__{}", migration.version, migration.name));
    }

    report.version = current_version(client)?;
    Ok(report)
}

/// The version the database is at, or 0 when nothing has been applied.
pub fn current_version(client: &mut Client) -> Result<i64> {
    let row = client
        .query_one(
            "SELECT coalesce(max(version), 0) FROM schema_migration",
            &[],
        )
        .context("reading the current schema version")?;
    Ok(row.get(0))
}

/// Refuses to continue unless the database is at [`EXPECTED_VERSION`].
///
/// The service calls this before it does anything else, so a schema mismatch
/// surfaces as a startup failure naming both versions rather than as a query
/// error against a column that has changed meaning.
pub fn assert_expected_version(client: &mut Client) -> Result<()> {
    let found = current_version(client)?;
    if found != EXPECTED_VERSION {
        bail!(
            "database schema is at version {found}, this build expects {EXPECTED_VERSION}. \
             Run `uaas migrate` if the database is behind; deploy a matching build if it is ahead."
        );
    }
    Ok(())
}

fn read_recorded(client: &mut Client) -> Result<Vec<(i64, String, Vec<u8>)>> {
    let rows = client
        .query(
            "SELECT version, name, checksum FROM schema_migration ORDER BY version",
            &[],
        )
        .context("reading schema_migration")?;
    Ok(rows
        .iter()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
        .collect())
}

/// Checks the database's history against this build's before applying anything.
fn verify(recorded: &[(i64, String, Vec<u8>)]) -> Result<()> {
    for (version, name, stored) in recorded {
        let Some(migration) = MIGRATIONS.iter().find(|m| m.version == *version) else {
            return Err(anyhow!(
                "database has migration V{version}__{name} applied, which this build does not \
                 contain. The database is ahead of the binary; deploy a matching build."
            ));
        };
        if checksum(migration.sql) != *stored {
            return Err(anyhow!(
                "migration V{version}__{} has changed since it was applied. An applied migration \
                 is history and cannot be edited; add a new migration instead.",
                migration.name
            ));
        }
    }
    Ok(())
}

/// One migration, in one transaction, with its bookkeeping row.
///
/// The row is written inside the same transaction as the DDL, so the two cannot
/// disagree: either the objects exist and the row says so, or neither happened.
fn apply(client: &mut Client, migration: &Migration) -> Result<()> {
    let mut tx: Transaction = client.transaction()?;
    tx.batch_execute(migration.sql)?;
    tx.execute(
        "INSERT INTO schema_migration (version, name, checksum) VALUES ($1, $2, $3)",
        &[
            &migration.version,
            &migration.name,
            &checksum(migration.sql),
        ],
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These need a PostgreSQL server and are skipped, not failed, without one.
    ///
    /// Each test gets its own schema rather than sharing `public`. The first
    /// version of this reset `public` per test, which passed serially and
    /// failed five ways under `cargo test`'s default parallelism — the tests
    /// were dropping each other's tables. Isolating them is the fix; running
    /// the suite single-threaded would only have hidden it and slowed every
    /// other test down.
    ///
    /// The schema is dropped and recreated on entry, so a rerun after a failure
    /// starts clean rather than inheriting the wreckage.
    fn client_for(test_name: &str) -> Option<Client> {
        let Ok(url) = std::env::var("UAAS_TEST_POSTGRES_URL") else {
            eprintln!("skipping {test_name}: UAAS_TEST_POSTGRES_URL not set");
            return None;
        };
        let mut client = Client::connect(&url, postgres::NoTls).expect("connect to postgres");
        let schema = schema_for(test_name);
        client
            .batch_execute(&format!(
                "DROP SCHEMA IF EXISTS {schema} CASCADE; \
                 CREATE SCHEMA {schema}; \
                 SET search_path TO {schema};"
            ))
            .expect("create an isolated schema");
        Some(client)
    }

    /// A schema name derived from the test name. Postgres identifiers are
    /// capped at 63 bytes, and the test names here are longer than that.
    fn schema_for(test_name: &str) -> String {
        let stem: String = test_name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(48)
            .collect();
        format!("t_{stem}")
    }

    fn table_names(client: &mut Client) -> Vec<String> {
        client
            .query(
                "SELECT tablename FROM pg_tables \
                 WHERE schemaname = current_schema() ORDER BY tablename",
                &[],
            )
            .expect("list tables")
            .iter()
            .map(|row| row.get(0))
            .collect()
    }

    // The embedded list must be contiguous, sorted, and cover every file on
    // disk. A file added but not registered would be silently skipped, which is
    // the one failure mode a versioned runner must not have.
    #[test]
    fn mig01_the_embedded_list_matches_the_files_on_disk() {
        for (i, migration) in MIGRATIONS.iter().enumerate() {
            assert_eq!(
                migration.version,
                i as i64 + 1,
                "migrations must be contiguous and sorted from 1"
            );
        }
        assert_eq!(
            MIGRATIONS.last().map(|m| m.version),
            Some(EXPECTED_VERSION),
            "EXPECTED_VERSION must name the last migration"
        );

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .expect("read migrations directory")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".sql"))
            .collect();
        on_disk.sort();

        let mut embedded: Vec<String> = MIGRATIONS
            .iter()
            .map(|m| format!("V{}__{}.sql", m.version, m.name))
            .collect();
        embedded.sort();

        assert_eq!(
            embedded, on_disk,
            "every .sql file must be registered in MIGRATIONS, and vice versa"
        );
    }

    #[test]
    fn mig02_applying_to_an_empty_database_creates_the_whole_schema() {
        let Some(mut client) = client_for("mig02_applying_to_an_empty_database") else {
            return;
        };

        let report = run(&mut client).expect("migrations apply");
        assert_eq!(report.applied.len(), MIGRATIONS.len());
        assert_eq!(report.already_applied, 0);
        assert_eq!(report.version, EXPECTED_VERSION);

        let mut expected: Vec<&str> = vec![
            "addr",
            "blocks",
            "collection",
            "connect",
            "mempool",
            "orphans",
            "schema_migration",
            "tx",
            "utxo",
            "utxo_monitor",
            "utxo_spent",
        ];
        expected.sort();
        assert_eq!(table_names(&mut client), expected);

        assert_expected_version(&mut client).expect("version matches after a full run");
    }

    #[test]
    fn mig03_a_second_run_applies_nothing() {
        let Some(mut client) = client_for("mig03_a_second_run_applies_nothing") else {
            return;
        };

        run(&mut client).expect("first run");
        let before = table_names(&mut client);

        let report = run(&mut client).expect("second run");
        assert!(
            report.applied.is_empty(),
            "a second run must apply nothing, applied {:?}",
            report.applied
        );
        assert_eq!(report.already_applied, MIGRATIONS.len());
        assert_eq!(table_names(&mut client), before);
    }

    // An applied migration is history. Editing the file must be refused, not
    // ignored, or the database and the repository disagree silently.
    #[test]
    fn mig04_an_edited_migration_is_refused() {
        let Some(mut client) = client_for("mig04_an_edited_migration_is_refused") else {
            return;
        };
        run(&mut client).expect("first run");

        client
            .execute(
                "UPDATE schema_migration SET checksum = $1 WHERE version = 5",
                &[&vec![0u8; 32]],
            )
            .expect("corrupt the recorded checksum");

        let err = run(&mut client).expect_err("a checksum mismatch must be refused");
        let message = format!("{err:#}");
        assert!(
            message.contains("V5__utxo") && message.contains("changed since it was applied"),
            "the error must name the file: {message}"
        );
    }

    // Rolling the binary back without rolling the schema back is a real
    // deployment mistake and must fail loudly.
    #[test]
    fn mig05_a_database_ahead_of_the_binary_is_refused() {
        let Some(mut client) = client_for("mig05_a_database_ahead_of_the_binary") else {
            return;
        };
        run(&mut client).expect("first run");

        client
            .execute(
                "INSERT INTO schema_migration (version, name, checksum) VALUES ($1, $2, $3)",
                &[&(EXPECTED_VERSION + 1), &"from_the_future", &vec![1u8; 32]],
            )
            .expect("record a migration this build does not have");

        let err = run(&mut client).expect_err("a database ahead of the binary must be refused");
        let message = format!("{err:#}");
        assert!(
            message.contains("ahead of the binary"),
            "the error must say which way round it is: {message}"
        );

        assert!(
            assert_expected_version(&mut client).is_err(),
            "the startup assertion must also refuse"
        );
    }

    // The property that lets the runner be this small: a failed migration
    // leaves neither its objects nor a row claiming it succeeded.
    #[test]
    fn mig06_a_failing_migration_leaves_nothing_behind() {
        let Some(mut client) = client_for("mig06_a_failing_migration_leaves_nothing_behind") else {
            return;
        };
        client
            .batch_execute(CREATE_BOOKKEEPING)
            .expect("bookkeeping table");

        let broken = Migration {
            version: 99,
            name: "broken",
            // Valid DDL followed by a statement that cannot run.
            sql: "CREATE TABLE half_applied (x integer); SELECT 1 / 0;",
        };

        apply(&mut client, &broken).expect_err("the migration must fail");

        let tables = table_names(&mut client);
        assert!(
            !tables.iter().any(|t| t == "half_applied"),
            "the table from the failed migration must not exist: {tables:?}"
        );
        assert_eq!(
            current_version(&mut client).expect("version"),
            0,
            "no bookkeeping row may survive a failed migration"
        );
    }

    #[test]
    fn mig07_the_startup_assertion_refuses_an_unmigrated_database() {
        let Some(mut client) = client_for("mig07_the_startup_assertion_refuses") else {
            return;
        };
        client
            .batch_execute(CREATE_BOOKKEEPING)
            .expect("bookkeeping table");

        let err = assert_expected_version(&mut client)
            .expect_err("an empty database must not satisfy the assertion");
        let message = format!("{err:#}");
        assert!(
            message.contains("version 0") && message.contains("expects 10"),
            "the error must name both versions: {message}"
        );
    }
}
