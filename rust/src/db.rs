//! PostgreSQL connection pooling.
//!
//! The `mysql` crate this replaced had a pool built in. `postgres` does not, so
//! the pool is assembled here and the rest of the crate names these aliases
//! rather than `r2d2_postgres` types directly — swapping the pool later then
//! touches one file.
//!
//! # No TLS
//!
//! Connections are made with [`NoTls`]. Every deployment today reaches the
//! database over loopback or a compose network, and the credentials are
//! development ones committed to the repository. Adding TLS means
//! `postgres-native-tls` and a certificate story, and is worth doing the moment
//! this talks to a database it does not share a host with. It is called out
//! here rather than left implicit in a type parameter.

use anyhow::{Context, Result};
use postgres::NoTls;
use r2d2_postgres::PostgresConnectionManager;

pub type Manager = PostgresConnectionManager<NoTls>;
pub type Pool = r2d2::Pool<Manager>;
pub type PooledConn = r2d2::PooledConnection<Manager>;

/// Builds a pool from a libpq connection URL.
///
/// r2d2 opens one connection eagerly to check the configuration, so a bad URL
/// or an unreachable server fails here rather than at the first query.
///
/// **Neither this nor [`Pool::get`] may be called from inside an async
/// runtime.** The synchronous postgres client drives a runtime of its own, and
/// nesting one panics with "Cannot start a runtime from within a runtime" —
/// then panics again in the client's destructor during cleanup, which makes it
/// a non-unwinding abort rather than a failed request.
///
/// `get` counts because r2d2 validates a connection as it hands it out:
/// `PostgresConnectionManager::is_valid` issues a query on the calling thread.
///
/// `web::block` is **not** a way round this. Tokio's blocking-pool threads
/// still carry the runtime context, so they panic exactly as a worker thread
/// would; see `rest_api::check_database`, which spawns a plain thread. `main`
/// does all of its database work — this call, the schema check and `Logic`
/// construction — before entering a runtime at all, and the peer manager runs
/// on a plain thread. A test that needs a pool inside `#[actix_web::test]` has
/// to build it on a plain thread too.
/// A connection URL with its password replaced, for an error message.
///
/// `build_pool` used to put the URL verbatim into its context, so a malformed
/// URL wrote the database password into an `error!` line — which is exactly
/// the path most likely to be hit and copied into a bug report (CS-450).
///
/// Purely textual: it finds the `user:password@` of the authority and blanks
/// the password. Anything it does not recognise is returned unchanged, which
/// is safe here because a string with no `user:password@` has no password to
/// leak.
pub(crate) fn redact_url(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let start = scheme_end + 3;
    // The authority ends at the first '/', '?' or '#'; a later '@' belongs to
    // the path or query and is not a credential separator.
    let end = url[start..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |i| start + i);
    // Last '@' rather than first: a password may itself contain one.
    let Some(at) = url[start..end].rfind('@') else {
        return url.to_string();
    };
    let at = start + at;
    let Some(colon) = url[start..at].find(':') else {
        // user@host with no password.
        return url.to_string();
    };
    let colon = start + colon;
    format!("{}:***{}", &url[..colon], &url[at..])
}

pub fn build_pool(url: &str) -> Result<Pool> {
    let config = url
        .parse()
        .with_context(|| format!("invalid PostgreSQL connection URL: {}", redact_url(url)))?;
    let manager = PostgresConnectionManager::new(config, NoTls);
    Pool::new(manager).context("could not connect to PostgreSQL")
}

/// One pool for the whole test binary.
///
/// `build_pool` fills a new pool to its maximum size eagerly, so a pool per
/// test multiplies quickly: `cargo test` runs as many tests at once as there
/// are cores, and twelve cores times ten connections is past a stock
/// PostgreSQL's `max_connections` of 100. The suite then fails as a whole with
/// "could not connect", which looks nothing like what it is.
///
/// Built once, shared, and sized for several `TxAnalyser`s at a time — each one
/// holds four connections for its lifetime, so a pool of ten would deadlock on
/// three concurrent tests rather than merely being slow.
///
/// `None` when `UAAS_TEST_POSTGRES_URL` is unset, which is a skip, not a
/// failure.
#[cfg(test)]
pub(crate) fn shared_test_pool() -> Option<Pool> {
    use std::sync::OnceLock;
    static POOL: OnceLock<Option<Pool>> = OnceLock::new();
    POOL.get_or_init(|| {
        let url = std::env::var("UAAS_TEST_POSTGRES_URL").ok()?;
        let config = url.parse().ok()?;
        let manager = PostgresConnectionManager::new(config, NoTls);
        Pool::builder().max_size(32).build(manager).ok()
    })
    .clone()
}

#[cfg(test)]
mod tests {
    use super::redact_url;

    /// The shape that actually appears in this repository's configs, and the
    /// one that used to reach an `error!` line intact.
    #[test]
    fn redact01_a_password_is_removed_from_a_libpq_url() {
        assert_eq!(
            redact_url("postgresql://uaas:uaas-password@localhost:5433/uaas_db"),
            "postgresql://uaas:***@localhost:5433/uaas_db"
        );
    }

    /// The host, port, database and user are all kept: the message has to stay
    /// useful for diagnosing a wrong target, which is its whole purpose.
    #[test]
    fn redact02_everything_that_is_not_the_password_survives() {
        let out = redact_url("postgresql://uaas:hunter2@db.internal:5432/uaas_db?sslmode=require");
        assert!(out.contains("uaas@") || out.contains("uaas:***@"), "{out}");
        assert!(out.contains("db.internal:5432/uaas_db"), "{out}");
        assert!(out.contains("sslmode=require"), "{out}");
        assert!(!out.contains("hunter2"), "the password survived: {out}");
    }

    /// Nothing to redact must not corrupt the string, or a malformed-URL error
    /// becomes unreadable on top of being an error.
    #[test]
    fn redact03_urls_without_a_password_are_unchanged() {
        for url in [
            "postgresql://localhost/uaas_db",
            "postgresql://uaas@localhost/uaas_db",
            "not a url at all",
            "",
        ] {
            assert_eq!(redact_url(url), url, "changed a url with no password");
        }
    }

    /// An `@` after the authority belongs to the path or query. Treating the
    /// last one in the whole string as the credential separator would blank
    /// most of the URL and hide the fault being reported.
    #[test]
    fn redact04_an_at_sign_later_in_the_url_is_not_a_credential() {
        assert_eq!(
            redact_url("postgresql://localhost/db?options=user@thing"),
            "postgresql://localhost/db?options=user@thing"
        );
        // A password containing '@' still goes, because the authority is
        // scanned from its right-hand end.
        let out = redact_url("postgresql://uaas:pa@ss@localhost/uaas_db");
        assert!(!out.contains("pa@ss"), "{out}");
        assert!(out.contains("localhost/uaas_db"), "{out}");
    }
}
