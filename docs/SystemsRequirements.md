# Systems Requirements

This document defines systems requirements for **UTXO as a Service (UaaS)** and maps each requirement to automated verification. Every requirement has at least one automated test (unit, integration, source-contract, or CI workflow check).

**Version:** 1.3.0  
**Related docs:** [Configuration](Configuration.md), [Security](Security.md), [Development](Development.md)

---

## 1. Scope

UaaS indexes the Bitcoin SV (BSV) blockchain by:

1. Connecting to BSV peer nodes over P2P (Rust service).
2. Maintaining blocks, transactions, mempool, and UTXO state in MariaDB/MySQL.
3. Exposing query, broadcast, and collection-monitor APIs via a Python FastAPI layer.

---

## 2. Verification methods

| Code | Method | Command / location |
|------|--------|-------------------|
| **AUT-R** | Rust unit / Actix tests | `cd rust && cargo test` |
| **AUT-P** | Python unit / smoke tests | `uv run pytest python/tests --ignore=python/tests/integration` |
| **AUT-I** | Python integration tests | `UAAS_TEST_MYSQL_URL=... uv run pytest python/tests/integration` |
| **AUT-S** | Source-contract tests | `python/tests/test_requirements_source.py` |
| **CI** | GitHub Actions | `.github/workflows/ci.yml` (verified by `python/tests/test_ci.py`) |

---

## 3. Requirements and verification

### 3.1 Configuration and startup

| ID | Requirement | Tests |
|----|-------------|-------|
| CFG-01 | Load configuration from TOML | AUT-R `cfg01_reads_config_from_toml_file`; AUT-P `test_cfg01_loads_valid_toml` |
| CFG-02 | Fail fast on missing/invalid config | AUT-P `test_cfg02_rejects_missing_service_section`, `test_cfg02_rejects_invalid_rate_limit` |
| CFG-03 | Use `mysql_url_docker` when `APP_ENV=docker` | AUT-R `cfg03_uses_docker_mysql_url_when_app_env_set` |
| CFG-04 | Read active network settings (IPs, ports) | AUT-R `cfg04_reads_active_network_port_and_ips`; AUT-P `test_cfg04_reads_active_network_settings` |
| CFG-05 | Load static `[[collection]]` monitors | AUT-P `test_cfg05_loads_static_collections`; AUT-I `test_get_collections` |
| CFG-06 | Persist dynamic monitors to configured file | AUT-R `cfg06_add_monitor_persists_to_dynamic_config_file`; AUT-P `test_cfg06_loads_dynamic_monitors_from_file` |

### 3.2 P2P synchronisation and indexing

| ID | Requirement | Tests |
|----|-------------|-------|
| SYNC-01 | Connect to BSV peers on network port | AUT-S `test_sync01_peer_connection_uses_configured_network_port`; AUT-R `cfg04_reads_active_network_port_and_ips` |
| SYNC-02 | Cycle to next IP on disconnect | AUT-S `test_sync02_thread_manager_cycles_configured_ips`; AUT-R `sync02_config_provides_multiple_peer_ips_for_failover` |
| SYNC-03 | Queue out-of-order blocks | AUT-S `test_sync03_out_of_order_blocks_are_queued` |
| SYNC-04 | Update mempool and UTXO on unconfirmed tx | AUT-I `test_sync04_mempool_table_accepts_transaction_row`; AUT-I `test_get_balance_sums_satoshi_by_confirmation` |
| SYNC-05 | Confirm txs and store block headers on block receipt | AUT-I `test_block_height_roundtrip`, `test_status_returns_database_counts` |
| SYNC-06 | Reach `Ready` state at chain tip | AUT-R `sync06_ready_state_reports_caught_up` |
| SYNC-07 | Orphan detection when enabled | AUT-P `test_sync07_orphan_detection_flag_is_configurable` |
| SYNC-08 | Startup load from DB or block file per config | AUT-R `sync08_startup_load_flag_available_per_network`; AUT-P `test_cfg04_reads_active_network_settings` |
| SYNC-09 | Log connect/disconnect to `connect` table | AUT-S `test_sync09_connect_events_are_logged`; AUT-I `test_sync09_connect_table_accepts_events` |
| SYNC-10 | Capture pattern-matched txs in `collection` | AUT-R `sync10_matches_locking_script_pattern`; AUT-P `test_sync10_collection_stores_monitor_names_in_memory` |

### 3.3 Python REST API — query

| ID | Requirement | Tests |
|----|-------------|-------|
| API-01 | `GET /` returns API metadata | AUT-P `test_root` |
| API-02 | `GET /health` 200/503 based on DB | AUT-P `test_health_when_database_*`; AUT-I `test_health_with_database` |
| API-03 | `GET /status` returns counts and Rust version | AUT-P `test_get_status_includes_database_counts`; AUT-I `test_status_*` |
| API-04 | `GET /utxo/get` returns UTXOs | AUT-P `test_utxo_returns_empty_list_for_valid_address`; AUT-I `test_utxo_empty_for_address` |
| API-05 | `GET /utxo/balance` confirmed/unconfirmed split | AUT-P `test_balance_returns_zero_for_valid_address`; AUT-I `test_balance_splits_confirmed_and_unconfirmed` |
| API-06 | Block query endpoints read `blocks` table | AUT-I `test_block_*` |
| API-07 | `/block/last*` returns 503 when empty | AUT-P `test_block_last_returns_503_when_no_blocks`; AUT-I `test_block_last_returns_503_when_empty` |
| API-08 | `/tx*` from block file or collection | AUT-P `test_unknown_tx_*`, `test_api08_returns_tx_from_collection_when_not_save_blocks` |
| API-09 | `GET /tx/proof` returns merkle branches | AUT-P `test_api09_create_merkle_branch_returns_left_right_positions`; AUT-I `test_api09_tx_proof_returns_merkle_branches` |
| API-10 | `GET /collection` lists monitors | AUT-I `test_get_collections` |
| API-11 | Invalid inputs return HTTP 422 | AUT-P `test_invalid_*`, `test_validation.py` |

### 3.4 Broadcast and collection monitors

| ID | Requirement | Tests |
|----|-------------|-------|
| BCAST-01 | Validate broadcast hex | AUT-P `test_validation.py` (`validate_broadcast_tx_hex`) |
| BCAST-02 | Reject oversized broadcast txs | AUT-P `test_oversized_broadcast_tx_returns_422`; AUT-R `broadcast_limits::tx_hex_*` |
| BCAST-03 | Reject duplicate txs with 422 | AUT-P `test_bcast03_rejects_duplicate_transaction` |
| BCAST-04 | Proxy valid txs to Rust `/tx/raw` | AUT-P `test_bcast04_proxies_valid_transaction_to_rust`, `test_broadcast_tx_hex_returns_503_when_rust_unreachable` |
| BCAST-05 | Rust decodes, limits, and queues broadcast | AUT-R `bcast05_broadcast_tx_queues_valid_transaction`, `broadcast_tx_requires_api_key_when_configured` |
| MON-01 | Add dynamic monitor via Rust | AUT-P `test_mon01_adds_dynamic_monitor_via_rust` |
| MON-02 | Reject duplicate monitor names | AUT-P `test_add_monitor_rejects_duplicate_name` |
| MON-03 | Reject deleting static monitors | AUT-P `test_delete_monitor_rejects_static_collection` |
| MON-04 | Reject unknown monitor on delete | AUT-P `test_delete_monitor_rejects_unknown_name` |

### 3.5 Rust internal REST API

| ID | Requirement | Tests |
|----|-------------|-------|
| RAPI-01 | `GET /health` checks DB and returns version | AUT-R `health_returns_ok_with_database` |
| RAPI-02 | `GET /health` returns 503 when DB unreachable | AUT-R `health_returns_503_when_database_unreachable` |
| RAPI-03 | `GET /version` returns package version | AUT-R `version_returns_package_version` |
| RAPI-04 | `POST /tx/raw` requires API key when configured | AUT-R `broadcast_tx_requires_api_key_when_configured` |
| RAPI-05 | Payload limit scales with broadcast max size | AUT-R `rapi05_payload_limit_scales_with_broadcast_max` |

### 3.6 Security and access control

| ID | Requirement | Tests |
|----|-------------|-------|
| SEC-01 | Python API key enforcement | AUT-P `test_protected_endpoint_*`, `test_health_does_not_require_api_key` |
| SEC-02 | Rust mutating endpoints require API key | AUT-R `broadcast_tx_requires_api_key_when_configured`, `health_does_not_require_api_key` |
| SEC-03 | Rate limit returns 429 | AUT-P `test_rate_limit_returns_429`, `test_rate_limit.py`; AUT-R `rate_limit.rs` tests |
| SEC-04 | `/health` exempt from rate limiting | AUT-P `test_health_exempt_from_rate_limit`; AUT-R `sec04_health_is_exempt_from_rate_limit` |
| SEC-05 | Rate limit uses `X-Forwarded-For` | AUT-P `test_uses_forwarded_for` |
| SEC-06 | Parameterized SQL | AUT-I `test_parameterized_query` |
| SEC-07 | Input validation rejects malformed data | AUT-P `test_validation.py`, smoke 422 tests |

### 3.7 Reliability and lifecycle

| ID | Requirement | Tests |
|----|-------------|-------|
| REL-01 | Graceful shutdown on Ctrl+C / SIGTERM | AUT-S `test_rel01_shutdown_sends_stop_to_peer_manager`; AUT-R `rel01_stop_event_is_used_for_shutdown` |
| REL-02 | Thread panics do not exit other threads | AUT-R `thread_util.rs` tests |
| REL-03 | Startup errors return non-zero exit | AUT-R `rel03_validate_startup_rejects_empty_ip_list` |
| REL-04 | Failed peer thread creation is non-fatal | AUT-S `test_rel04_failed_peer_connection_is_logged_not_fatal` |

### 3.8 Deployment and operations

| ID | Requirement | Tests |
|----|-------------|-------|
| OPS-01 | Docker Compose defines all services | AUT-P `test_ops01_compose_defines_all_services` |
| OPS-02 | Database health check | AUT-P `test_ops02_database_has_healthcheck` |
| OPS-03 | Rust backend health check | AUT-P `test_ops03_backend_healthcheck_targets_rust_health` |
| OPS-04 | Python web health check | AUT-P `test_ops04_web_healthcheck_targets_python_health` |
| OPS-05 | Shared config/data volumes | AUT-P `test_ops05_application_services_mount_shared_data` |
| OPS-06 | Core DB tables exist | AUT-I `test_blocks_table_exists`, `test_ops06_core_tables_exist` |
| OPS-07 | CI runs Rust fmt/clippy/test | AUT-P `test_ops07_ci_runs_rust_checks`; CI workflow |
| OPS-08 | CI runs Python lint/test | AUT-P `test_ops08_ci_runs_python_checks`; CI workflow |
| OPS-09 | CI builds Docker images | AUT-P `test_ops09_ci_builds_docker_images`; CI workflow |

### 3.9 Data integrity

| ID | Requirement | Tests |
|----|-------------|-------|
| DATA-01 | No duplicate block headers in `blocks` | AUT-I `test_data01_duplicate_block_hash_rejected` |
| DATA-02 | UTXO maintained on spend/create | AUT-I `test_balance_splits_confirmed_and_unconfirmed`, `test_get_utxo_returns_matching_rows` |
| DATA-03 | Mempool stores fee and time metadata | AUT-I `test_data03_mempool_table_has_fee_and_time_columns`; AUT-I `test_sync04_mempool_table_accepts_transaction_row` |
| DATA-04 | Block file offsets locate serialized blocks | AUT-P `test_data04_block_offset_locates_serialized_block` |

---

## 4. Running verification

```bash
# Rust
export UAAS_TEST_MYSQL_URL=mysql://maas:maas-password@127.0.0.1:3306/main_uaas_db
cd rust && cargo fmt --check && cargo clippy && cargo test

# Python unit + smoke + source-contract + CI checks
uv sync --all-groups
./lint.sh
uv run pytest python/tests --ignore=python/tests/integration -v

# Python integration (requires MariaDB)
export UAAS_TEST_MYSQL_URL=mysql://maas:maas-password@127.0.0.1:3306/main_uaas_db
uv run pytest python/tests/integration -v
```

See [Development](Development.md) for full setup instructions.

---

## 5. Test file index

| File | Requirements covered |
|------|---------------------|
| `rust/src/config.rs` (tests) | CFG-01, CFG-03, CFG-04, REL-03, SYNC-02, SYNC-08 |
| `rust/src/dynamic_config.rs` (tests) | CFG-06 |
| `rust/src/rest_api.rs` (tests) | BCAST-02, BCAST-05, RAPI-01–05, SEC-02, SEC-04 |
| `rust/src/rate_limit.rs` (tests) | SEC-03 |
| `rust/src/thread_util.rs` (tests) | REL-02 |
| `rust/src/peer_event.rs` (tests) | REL-01 |
| `rust/src/uaas/collection.rs` (tests) | SYNC-10 |
| `rust/src/uaas/logic.rs` (tests) | SYNC-06 |
| `python/tests/test_config.py` | CFG-01, CFG-02, CFG-04, CFG-05, SYNC-07 |
| `python/tests/test_collection.py` | CFG-06, API-08, SYNC-10 |
| `python/tests/test_deployment.py` | OPS-01–05 |
| `python/tests/test_ci.py` | OPS-07–09 |
| `python/tests/test_merkle.py` | API-09 |
| `python/tests/test_blockfile.py` | DATA-04 |
| `python/tests/test_requirements_source.py` | SYNC-01–03, SYNC-09, REL-01, REL-04 |
| `python/tests/test_rest_api_smoke.py` | API-*, BCAST-*, MON-*, SEC-* |
| `python/tests/integration/test_system_requirements.py` | API-09, DATA-01, DATA-03, OPS-06, SYNC-09 |
| `python/tests/integration/test_mempool_integration.py` | SYNC-04, DATA-03 |
| `python/tests/integration/test_rest_api_integration.py` | API-*, SYNC-05, DATA-02 |
