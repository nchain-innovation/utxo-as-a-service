use mysql::{prelude::*, PooledConn};

fn table_exists(conn: &mut PooledConn, table: &str) -> bool {
    let query = format!(
        "SELECT COUNT(*) FROM INFORMATION_SCHEMA.TABLES \
         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table}'"
    );
    conn.query_first::<i64, _>(&query)
        .ok()
        .flatten()
        .unwrap_or(0)
        > 0
}

fn ensure_index(conn: &mut PooledConn, table: &str, ddl: &str) {
    if !table_exists(conn, table) {
        return;
    }
    if let Err(err) = conn.query_drop(ddl) {
        log::warn!("Unable to ensure index on {table}: {err:?}");
    }
}

fn ensure_mempool_primary_key(conn: &mut PooledConn) {
    if !table_exists(conn, "mempool") {
        return;
    }

    let has_pk = conn
        .query_first::<i64, _>(
            "SELECT COUNT(*) FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS \
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'mempool' \
             AND CONSTRAINT_TYPE = 'PRIMARY KEY'",
        )
        .ok()
        .flatten()
        .unwrap_or(0)
        > 0;

    if has_pk {
        return;
    }

    log::info!("Migrating mempool table: adding primary key on hash");

    if let Err(err) = conn.query_drop(
        "DELETE m1 FROM mempool m1 \
         INNER JOIN mempool m2 ON m1.hash = m2.hash AND m1.time > m2.time",
    ) {
        log::warn!("Unable to deduplicate mempool rows before primary key migration: {err:?}");
    }

    if let Err(err) = conn.query_drop("DROP INDEX IF EXISTS idx_txkey ON mempool") {
        log::warn!("Unable to drop legacy mempool hash index: {err:?}");
    }

    if let Err(err) = conn.query_drop("ALTER TABLE mempool ADD PRIMARY KEY (hash)") {
        log::error!("Unable to add mempool primary key: {err:?}");
    }
}

/// Apply height indexes and mempool primary key for existing and new databases.
pub fn ensure_performance_indexes(conn: &mut PooledConn) {
    ensure_index(
        conn,
        "blocks",
        "CREATE INDEX IF NOT EXISTS idx_blocks_height ON blocks (height)",
    );
    ensure_index(
        conn,
        "tx",
        "CREATE INDEX IF NOT EXISTS idx_tx_height_blockindex ON tx (height, blockindex)",
    );
    ensure_index(
        conn,
        "utxo",
        "CREATE INDEX IF NOT EXISTS idx_utxo_height ON utxo (height)",
    );
    ensure_mempool_primary_key(conn);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perf_indexes_apply_on_test_database() {
        let Some(url) = std::env::var("UAAS_TEST_MYSQL_URL").ok() else {
            eprintln!("skipping perf_indexes_apply_on_test_database: UAAS_TEST_MYSQL_URL not set");
            return;
        };

        let pool = mysql::Pool::new(url.as_str()).expect("connect to UAAS_TEST_MYSQL_URL");
        let mut conn = pool
            .get_conn()
            .expect("get connection for schema migration test");

        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS blocks (
                height int unsigned not null,
                hash varchar(64) not null,
                version int unsigned not null,
                prev_hash varchar(64) not null,
                merkle_root varchar(64) not null,
                timestamp int unsigned not null,
                bits int unsigned not null,
                nonce int unsigned not null,
                `offset` bigint unsigned not null,
                blocksize int unsigned not null,
                numtxs int unsigned not null,
                PRIMARY KEY (hash)
            )",
        )
        .expect("create blocks table for schema test");

        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS tx (
                hash varchar(64) not null,
                height int unsigned not null,
                blockindex int unsigned not null,
                txsize int unsigned not null,
                satoshis bigint unsigned not null,
                PRIMARY KEY (hash)
            )",
        )
        .expect("create tx table for schema test");

        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS utxo (
                hash varchar(64) not null,
                pos int unsigned not null,
                satoshis bigint unsigned not null,
                height int not null,
                pubkeyhash varchar(64),
                PRIMARY KEY (hash, pos)
            )",
        )
        .expect("create utxo table for schema test");

        conn.query_drop("DROP TABLE IF EXISTS mempool")
            .expect("drop mempool");
        conn.query_drop(
            "CREATE TABLE mempool (
                hash varchar(64) not null,
                locktime int unsigned not null,
                fee bigint unsigned not null,
                time int unsigned not null,
                tx longtext not null
            )",
        )
        .expect("create legacy mempool table for schema test");
        conn.query_drop("CREATE INDEX idx_txkey ON mempool (hash)")
            .expect("create legacy mempool index");

        ensure_performance_indexes(&mut conn);

        let indexes: Vec<String> = conn
            .query(
                "SELECT INDEX_NAME FROM INFORMATION_SCHEMA.STATISTICS \
                 WHERE TABLE_SCHEMA = DATABASE() \
                 AND INDEX_NAME IN ('idx_blocks_height', 'idx_tx_height_blockindex', 'idx_utxo_height', 'PRIMARY') \
                 AND TABLE_NAME IN ('blocks', 'tx', 'utxo', 'mempool')",
            )
            .expect("query performance indexes");

        assert!(
            indexes.len() >= 4,
            "expected performance indexes to be present, found {indexes:?}"
        );

        conn.query_drop("DROP TABLE IF EXISTS mempool")
            .expect("cleanup mempool");
    }
}
