# Security

UaaS exposes HTTP APIs and a database that were designed for trusted network environments. Before deploying beyond local development, review the following.

## Network exposure

| Service | Default port | Notes |
|---------|--------------|-------|
| Python REST API | 5010 | Query UTXO data, broadcast transactions, manage monitors |
| Rust REST API | 8081 | Used internally by Python; also accepts broadcast and monitor changes |
| PostgreSQL | 5433 (compose) | Stores blocks, UTXO set, collections |
| Adminer (compose) | 8080 | Database admin UI with no built-in authentication |

**Recommendations:**

- Bind the Python API to `127.0.0.1` when running on a single host unless another layer (VPN, reverse proxy, firewall) restricts access.
- Do not publish ports `8081`, `3307`, or `8080` to the public internet.
- Supply the database URL through `UAAS_POSTGRES_URL` rather than editing `data/uaasr.toml`; see [Credentials](#credentials) below.

## Credentials

**This repository is public.** Anything committed here is disclosed the moment
it is pushed, and deleting it later does not remove it from git history — the
answer to a disclosed credential is to rotate it, not to try to un-publish it.

So the tracked application configs carry a placeholder rather than a working
database URL:

```toml
[database]
postgres_url = "postgresql://uaas:CHANGE-ME@localhost:5433/uaas_db"
```

Both components refuse to start on a `CHANGE-ME` value and say what to set,
rather than attempting a connection that would fail with something less
useful.

**Peer addresses are not treated this way.** A node's address is not a
credential — nodes gossip each other's addresses in `addr` messages and public
crawlers list reachable ones — and a placeholder there means the stack starts
and silently never syncs, which is worse for development. The testnet peer is
concrete; revisit it before a live deployment.

The **mainnet** peer list is `192.0.2.1` — TEST-NET-1 (RFC 5737), valid and
guaranteed not to route. Switching `network` to mainnet therefore cannot
silently start dialling a real node: the service starts, stays healthy and
warns that it will never sync. Put a real address there deliberately, when
that is what is wanted.

### Supplying real values

| What | How |
|------|-----|
| Database URL | `UAAS_POSTGRES_URL`. Read by `uaas migrate`, the Rust service and the Python API, in preference to the config file. `docker-compose.yml` sets it for all three. |
| Peer addresses | Edit the config, or supply the whole config as JSON in `UAASR_CONFIG`. Not secret; see above. |
| Everything else | An untracked config, or `UAASR_CONFIG`. |

An empty `UAAS_POSTGRES_URL` counts as unset and falls back to the file.

### What is still in the repository, deliberately

`docker-compose.yml` and `init_database/postgres/01_roles_and_databases.sql`
carry the development password for the throwaway PostgreSQL container that
compose creates. They have to agree with each other for the stack to
initialise itself, and that container is local-only and recreated from empty.

**Never reuse those values anywhere shared.** Removing them entirely means
templating the database bootstrap at deploy time, which has not been done; see
`.env.example`. `POSTGRES_PASSWORD` and `UAAS_POSTGRES_URL` can both be
overridden from `.env` today.

### Connection URLs in logs

A connection URL carries the password, so it is redacted before it reaches an
error message — `postgresql://uaas:***@host:5432/db`. If you add a log line on
a database path, do the same.

## Optional API key

Set `api_key` under `[web_interface]` in `data/uaasr.toml` to require the `X-API-Key` header on API requests.

```toml
[web_interface]
address = '127.0.0.1:5010'
api_key = "change-me-to-a-long-random-secret"
```

When enabled:

- All Python REST endpoints except `GET /health` require the header (Docker healthchecks continue to work).
- Rust mutating endpoints (`POST /tx/raw`, collection monitor add/delete) require the same header.
- Python forwards the key automatically when calling the Rust backend.

Example request:

```bash
curl -H "X-API-Key: change-me-to-a-long-random-secret" \
  http://127.0.0.1:5010/status
```

When `api_key` is omitted from config, authentication is disabled (default for local development).

## Rate limiting

Set `rate_limit_per_minute` under `[web_interface]` to cap requests per client IP (per minute). `0` disables limiting (default). `/health` is always exempt so Docker healthchecks keep working. When running behind a reverse proxy, ensure `X-Forwarded-For` reflects the real client address.

## Sensitive operations

Even with an API key, treat the service as privileged infrastructure:

- **Broadcast** (`POST /tx/hex`) relays transactions to the BSV network. Payload size is capped by `max_broadcast_tx_bytes` in `[web_interface]` (default 1 MiB) on both the Python and Rust APIs.
- **Collection monitors** can capture and store arbitrary matching transactions.
- **UTXO queries** reveal balance and transaction data for queried addresses.

Use TLS termination at a reverse proxy when traffic crosses untrusted networks. This project does not terminate HTTPS itself.

## Docker Compose

The sample `docker-compose.yml` uses weak default credentials for its own throwaway database and exposes Adminer for convenience. Treat it as a development stack, not a production deployment template. See [Credentials](#credentials) for what is a placeholder and what is not.
