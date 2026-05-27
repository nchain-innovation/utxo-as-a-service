-- Performance indexes for height-based queries and mempool lookups.
-- Safe to run on existing databases; Rust startup also applies these via schema.rs.
-- Replace `main_uaas_db` with your database name if different.

USE main_uaas_db;

CREATE INDEX IF NOT EXISTS idx_blocks_height ON blocks (height);
CREATE INDEX IF NOT EXISTS idx_tx_height_blockindex ON tx (height, blockindex);
CREATE INDEX IF NOT EXISTS idx_utxo_height ON utxo (height);

-- Migrate legacy mempool tables that only had idx_txkey on hash.
DELETE m1 FROM mempool m1
INNER JOIN mempool m2 ON m1.hash = m2.hash AND m1.time > m2.time;

DROP INDEX IF EXISTS idx_txkey ON mempool;

-- Ignore error 1068 (multiple primary key) if already migrated.
ALTER TABLE mempool ADD PRIMARY KEY (hash);
