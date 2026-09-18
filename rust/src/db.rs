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
pub fn build_pool(url: &str) -> Result<Pool> {
    let config = url
        .parse()
        .with_context(|| format!("invalid PostgreSQL connection URL: {url}"))?;
    let manager = PostgresConnectionManager::new(config, NoTls);
    Pool::new(manager).context("could not connect to PostgreSQL")
}
