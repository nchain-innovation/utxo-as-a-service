# Security

UaaS exposes HTTP APIs and a database that were designed for trusted network environments. Before deploying beyond local development, review the following.

## Network exposure

| Service | Default port | Notes |
|---------|--------------|-------|
| Python REST API | 5010 | Query UTXO data, broadcast transactions, manage monitors |
| Rust REST API | 8081 | Used internally by Python; also accepts broadcast and monitor changes |
| MariaDB | 3307 (compose) | Stores blocks, UTXO set, collections |
| Adminer (compose) | 8080 | Database admin UI with no built-in authentication |

**Recommendations:**

- Bind the Python API to `127.0.0.1` when running on a single host unless another layer (VPN, reverse proxy, firewall) restricts access.
- Do not publish ports `8081`, `3307`, or `8080` to the public internet.
- Replace default database passwords in `docker-compose.yml` and `data/uaasr.toml` for any shared or production environment.

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

## Sensitive operations

Even with an API key, treat the service as privileged infrastructure:

- **Broadcast** (`POST /tx/hex`) relays transactions to the BSV network.
- **Collection monitors** can capture and store arbitrary matching transactions.
- **UTXO queries** reveal balance and transaction data for queried addresses.

Use TLS termination at a reverse proxy when traffic crosses untrusted networks. This project does not terminate HTTPS itself.

## Docker Compose

The sample `docker-compose.yml` uses weak default credentials and exposes Adminer for convenience. Treat it as a development stack, not a production deployment template.
