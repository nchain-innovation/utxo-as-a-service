use std::io::Cursor;
use std::sync::{mpsc, Arc};

use actix_web::{
    delete, get, http::header::ContentType, post, web, HttpRequest, HttpResponse, Responder, Result,
};
use serde::Serialize;

use chain_gang::{messages::Tx, util::Serializable};

use crate::config::CollectionConfig;
use crate::db::Pool;
use crate::rate_limit::RateLimiter;
use crate::uaas::tx_bounds::validate_tx_bytes;
use crate::uaas::util::decode_hexstr;

// RestEventMessage - used for sending messages from REST API to main event processing loop

#[derive(PartialEq, Clone, Eq, Debug)]
pub enum RestEventMessage {
    TxForBroadcast(Tx),
    AddMonitor(CollectionConfig),
    DeleteMonitor(String),
}

// web interface state
pub struct AppState {
    pub msg_from_rest_api: mpsc::Sender<RestEventMessage>,
    pub api_key: Option<String>,
    pub rate_limiter: Arc<RateLimiter>,
    pub max_broadcast_tx_bytes: usize,
    pub db_pool: Pool,
}

fn tx_hex_exceeds_limit(hex_len: usize, max_tx_bytes: usize) -> bool {
    hex_len / 2 > max_tx_bytes
}

const API_KEY_HEADER: &str = "X-API-Key";

fn rate_limit(req: &HttpRequest, limiter: &RateLimiter) -> Option<HttpResponse> {
    if limiter.allow(&crate::rate_limit::client_ip(req)) {
        None
    } else {
        Some(HttpResponse::TooManyRequests().json(serde_json::json!({
            "failure": "Rate limit exceeded",
        })))
    }
}

fn authorize(req: &HttpRequest, api_key: &Option<String>) -> Option<HttpResponse> {
    let expected = api_key.as_ref()?;
    let authorized = req
        .headers()
        .get(API_KEY_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|provided| provided == expected);
    if authorized {
        None
    } else {
        Some(HttpResponse::Unauthorized().json(serde_json::json!({
            "failure": "Unauthorized",
        })))
    }
}

#[derive(Serialize)]
struct BroadcastTxResponse {
    status: String,
    detail: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    database: Option<String>,
}

/// Probes the database on a thread of its own.
///
/// This cannot run on an actix worker *or* on a `web::block` thread. The
/// synchronous postgres client drives a runtime internally, and tokio's
/// blocking pool threads still carry the runtime context, so `block_on` there
/// panics with "Cannot start a runtime from within a runtime". `mysql` had no
/// such constraint, which is why `web::block` alone used to be enough.
///
/// The peer-manager side is unaffected: it is a plain `thread::spawn` and was
/// never inside a runtime.
fn check_database(pool: &Pool) -> Result<(), String> {
    let pool = pool.clone();
    std::thread::spawn(move || probe(&pool))
        .join()
        .map_err(|_| "database health check thread panicked".to_string())?
}

fn probe(pool: &Pool) -> Result<(), String> {
    let mut conn = pool.get().map_err(|err| err.to_string())?;
    // query_one rather than query_opt: `SELECT 1` returning no row would mean
    // the server answered something other than a working connection, which is
    // exactly what this probe is for.
    let one: i32 = conn
        .query_one("SELECT 1", &[])
        .map_err(|err| err.to_string())?
        .get(0);
    if one != 1 {
        return Err("database health check returned an unexpected value".to_string());
    }
    Ok(())
}

#[get("/health")]
async fn health(data: web::Data<AppState>) -> impl Responder {
    let pool = data.db_pool.clone();
    match web::block(move || check_database(&pool)).await {
        Ok(Ok(())) => HttpResponse::Ok().json(HealthResponse {
            status: "ok",
            service: "uaas-service",
            version: Some(env!("CARGO_PKG_VERSION")),
            database: None,
        }),
        Ok(Err(db_err)) => HttpResponse::ServiceUnavailable().json(HealthResponse {
            status: "unhealthy",
            service: "uaas-service",
            version: None,
            database: Some(db_err),
        }),
        Err(block_err) => HttpResponse::ServiceUnavailable().json(HealthResponse {
            status: "unhealthy",
            service: "uaas-service",
            version: None,
            database: Some(block_err.to_string()),
        }),
    }
}

#[get("/version")]
async fn version(req: HttpRequest, data: web::Data<AppState>) -> impl Responder {
    if let Some(response) = rate_limit(&req, &data.rate_limiter) {
        return response;
    }

    log::info!("version");
    let version = env!("CARGO_PKG_VERSION");
    let status = format!("{{\"version\": \"{}\"}}", version);
    HttpResponse::Ok()
        .content_type(ContentType::json())
        .body(status)
}

// to test
// curl -X POST -d 'txt=txt' 127.0.0.1:8080/echo
#[post("/tx/raw")]
async fn broadcast_tx(
    hexstr: String,
    req: HttpRequest,
    data: web::Data<AppState>,
) -> Result<HttpResponse> {
    if let Some(response) = rate_limit(&req, &data.rate_limiter) {
        return Ok(response);
    }
    if let Some(response) = authorize(&req, &data.api_key) {
        return Ok(response);
    }

    if tx_hex_exceeds_limit(hexstr.len(), data.max_broadcast_tx_bytes) {
        return Ok(HttpResponse::Ok().json(BroadcastTxResponse {
            status: "Failed".to_string(),
            detail: format!(
                "Transaction exceeds maximum broadcast size of {} bytes",
                data.max_broadcast_tx_bytes
            ),
        }));
    }

    // decode the hexstr to tx
    let bytes = match decode_hexstr(&hexstr) {
        Ok(b) => b,
        Err(_) => {
            return Ok(HttpResponse::Ok().json(BroadcastTxResponse {
                status: "Failed".to_string(),
                detail: "Failed to decode hex".to_string(),
            }));
        }
    };

    // Bounds check before Tx::read. The deserialiser sizes its allocations from
    // varints in these bytes, and an oversized allocation aborts the process
    // rather than panicking, so there is nothing to catch downstream.
    if let Err(err) = validate_tx_bytes(&bytes) {
        log::warn!("Rejected malformed broadcast transaction: {err}");
        return Ok(HttpResponse::Ok().json(BroadcastTxResponse {
            status: "Failed".to_string(),
            detail: format!("Malformed transaction: {err}"),
        }));
    }

    let tx = match Tx::read(&mut Cursor::new(&bytes)) {
        Ok(tx) => tx,
        Err(_) => {
            return Ok(HttpResponse::Ok().json(BroadcastTxResponse {
                status: "Failed".to_string(),
                detail: "Failed to convert hex to tx".to_string(),
            }));
        }
    };

    let hash = tx.hash().encode();

    // Send Tx for broadcast
    if data
        .msg_from_rest_api
        .send(RestEventMessage::TxForBroadcast(tx))
        .is_err()
    {
        log::error!("REST API channel closed; cannot broadcast transaction");
        return Ok(HttpResponse::Ok().json(BroadcastTxResponse {
            status: "Failed".to_string(),
            detail: "Service unavailable".to_string(),
        }));
    }

    // Return hash as hex_str, if successful
    Ok(HttpResponse::Ok().json(BroadcastTxResponse {
        status: "Success".to_string(),
        detail: hash,
    }))
}

#[post("/collection/monitor")]
async fn add_monitor(
    monitor: web::Json<CollectionConfig>,
    req: HttpRequest,
    data: web::Data<AppState>,
) -> Result<impl Responder> {
    if let Some(response) = rate_limit(&req, &data.rate_limiter) {
        return Ok(response);
    }
    if let Some(response) = authorize(&req, &data.api_key) {
        return Ok(response);
    }

    log::info!("add_monitor");

    let cc = monitor.into_inner();

    if data
        .msg_from_rest_api
        .send(RestEventMessage::AddMonitor(cc))
        .is_err()
    {
        log::error!("REST API channel closed; cannot add monitor");
        return Ok(HttpResponse::ServiceUnavailable().body("Service unavailable"));
    }

    Ok(HttpResponse::Ok().finish())
}

#[delete("/collection/monitor/{monitor_name}")]
async fn delete_monitor(
    monitor_name: web::Path<String>,
    req: HttpRequest,
    data: web::Data<AppState>,
) -> Result<impl Responder> {
    if let Some(response) = rate_limit(&req, &data.rate_limiter) {
        return Ok(response);
    }
    if let Some(response) = authorize(&req, &data.api_key) {
        return Ok(response);
    }

    log::info!("delete_monitor '{}'", monitor_name);

    if data
        .msg_from_rest_api
        .send(RestEventMessage::DeleteMonitor(monitor_name.to_string()))
        .is_err()
    {
        log::error!("REST API channel closed; cannot delete monitor");
        return Ok(HttpResponse::ServiceUnavailable().body("Service unavailable"));
    }

    Ok(HttpResponse::Ok().finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test as actix_test, App};
    use std::sync::mpsc;

    fn postgres_test_url() -> Option<String> {
        std::env::var("UAAS_TEST_POSTGRES_URL").ok()
    }

    /// Builds a pool on a plain thread, once for the whole test binary.
    ///
    /// Two constraints, both from the synchronous postgres client driving a
    /// runtime of its own:
    ///
    /// * it cannot be *built* inside a runtime, because `Pool::new` connects
    ///   eagerly — and `#[actix_web::test]` is a runtime;
    /// * it cannot be *dropped* inside one either, because closing a client
    ///   blocks the same way.
    ///
    /// A `OnceLock` satisfies both: built once on a plain thread, and never
    /// dropped. In production neither arises — `main` builds the pool before
    /// entering the runtime and holds it for the life of the process.
    fn pool_off_runtime(url: String) -> Option<Pool> {
        std::thread::spawn(move || crate::db::build_pool(&url).ok())
            .join()
            .ok()?
    }

    fn live_db_pool() -> Option<Pool> {
        static POOL: std::sync::OnceLock<Option<Pool>> = std::sync::OnceLock::new();
        POOL.get_or_init(|| pool_off_runtime(postgres_test_url()?))
            .clone()
    }

    /// A pool whose password is wrong, built *without* connecting.
    ///
    /// `db::build_pool` goes through `r2d2::Pool::new`, which opens one
    /// connection eagerly and so returns `Err` on bad credentials — it can
    /// never hand back the broken pool these tests need. This fixture used to
    /// call it and silently skipped both tests as a result. `build_unchecked`
    /// skips that initial connection, so the failure lands where the health
    /// check actually meets it: at `pool.get()`.
    ///
    /// The short connection timeout is what keeps that failure to seconds.
    /// r2d2 retries a failing connection until the timeout elapses, and its
    /// default is 30 seconds.
    fn invalid_credentials_pool() -> Option<Pool> {
        let url = postgres_test_url()?;
        // Swap whatever password the URL carries for one that is not it.
        let bad_url = url.split_once(':').and_then(|(scheme, rest)| {
            rest.rsplit_once('@').map(|(creds, host)| {
                let user = creds
                    .trim_start_matches("//")
                    .split(':')
                    .next()
                    .unwrap_or("");
                format!("{scheme}://{user}:not-the-password@{host}")
            })
        })?;
        static POOL: std::sync::OnceLock<Option<Pool>> = std::sync::OnceLock::new();
        POOL.get_or_init(|| {
            let config = bad_url.parse().ok()?;
            let manager = crate::db::Manager::new(config, postgres::NoTls);
            Some(
                r2d2::Pool::builder()
                    .connection_timeout(std::time::Duration::from_secs(2))
                    .build_unchecked(manager),
            )
        })
        .clone()
    }

    fn skip_without_postgres(test_name: &str) -> Option<Pool> {
        live_db_pool().or_else(|| {
            eprintln!("skipping {test_name}: UAAS_TEST_POSTGRES_URL not set");
            None
        })
    }

    mod broadcast_limits {
        use super::tx_hex_exceeds_limit;

        #[test]
        fn tx_hex_within_limit() {
            assert!(!tx_hex_exceeds_limit(1_999_998, 1_000_000));
        }

        #[test]
        fn tx_hex_at_limit() {
            assert!(!tx_hex_exceeds_limit(2_000_000, 1_000_000));
        }

        #[test]
        fn tx_hex_over_limit() {
            assert!(tx_hex_exceeds_limit(2_000_002, 1_000_000));
        }
    }

    mod database_checks {
        use super::*;

        #[test]
        fn live_database_passes_health_check() {
            let Some(pool) = skip_without_postgres("live_database_passes_health_check") else {
                return;
            };
            check_database(&pool).expect("database health check should succeed");
        }

        #[test]
        fn invalid_credentials_fail_health_check() {
            let Some(pool) = invalid_credentials_pool() else {
                eprintln!(
                    "skipping invalid_credentials_fail_health_check: \
                     could not create pool with invalid credentials"
                );
                return;
            };
            let result = check_database(&pool);
            assert!(
                result.is_err(),
                "expected database check to fail: {result:?}"
            );
        }
    }

    fn test_app_state(db_pool: Pool) -> web::Data<AppState> {
        let (tx, _rx) = mpsc::channel();
        web::Data::new(AppState {
            msg_from_rest_api: tx,
            api_key: None,
            rate_limiter: Arc::new(RateLimiter::new(0)),
            max_broadcast_tx_bytes: 1_000_000,
            db_pool,
        })
    }

    #[actix_web::test]
    async fn health_returns_ok_with_database() {
        let Some(pool) = skip_without_postgres("health_returns_ok_with_database") else {
            return;
        };

        let app =
            actix_test::init_service(App::new().app_data(test_app_state(pool)).service(health))
                .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::get().uri("/health").to_request(),
        )
        .await;

        assert_eq!(response.status(), 200);
        let bytes = actix_test::read_body(response).await;
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("health json");
        assert_eq!(body["status"], "ok");
        assert_eq!(body["service"], "uaas-service");
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
        assert!(body.get("database").is_none());
    }

    #[actix_web::test]
    async fn health_returns_503_when_database_unreachable() {
        let Some(pool) = invalid_credentials_pool() else {
            eprintln!(
                "skipping health_returns_503_when_database_unreachable: \
                 could not create pool with invalid credentials"
            );
            return;
        };

        let app =
            actix_test::init_service(App::new().app_data(test_app_state(pool)).service(health))
                .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::get().uri("/health").to_request(),
        )
        .await;

        assert_eq!(response.status(), 503);
        let bytes = actix_test::read_body(response).await;
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("health json");
        assert_eq!(body["status"], "unhealthy");
        assert_eq!(body["service"], "uaas-service");
        assert!(body.get("version").is_none());
        assert!(body.get("database").is_some());
    }

    #[actix_web::test]
    async fn version_returns_package_version() {
        let Some(pool) = skip_without_postgres("version_returns_package_version") else {
            return;
        };

        let app =
            actix_test::init_service(App::new().app_data(test_app_state(pool)).service(version))
                .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::get().uri("/version").to_request(),
        )
        .await;

        assert_eq!(response.status(), 200);
        let body = actix_test::read_body(response).await;
        let body = std::str::from_utf8(&body).expect("version response should be utf-8");
        assert!(body.contains(env!("CARGO_PKG_VERSION")));
    }

    #[actix_web::test]
    async fn health_does_not_require_api_key() {
        let Some(pool) = skip_without_postgres("health_does_not_require_api_key") else {
            return;
        };

        let (tx, _rx) = mpsc::channel();
        let app = actix_test::init_service(
            App::new()
                .app_data(web::Data::new(AppState {
                    msg_from_rest_api: tx,
                    api_key: Some("secret-key".to_string()),
                    rate_limiter: Arc::new(RateLimiter::new(0)),
                    max_broadcast_tx_bytes: 1_000_000,
                    db_pool: pool,
                }))
                .service(health),
        )
        .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::get().uri("/health").to_request(),
        )
        .await;

        assert_ne!(response.status(), 401);
    }

    #[actix_web::test]
    async fn broadcast_tx_requires_api_key_when_configured() {
        let Some(pool) = skip_without_postgres("broadcast_tx_requires_api_key_when_configured")
        else {
            return;
        };

        let (tx, _rx) = mpsc::channel();
        let app = actix_test::init_service(
            App::new()
                .app_data(web::Data::new(AppState {
                    msg_from_rest_api: tx,
                    api_key: Some("secret-key".to_string()),
                    rate_limiter: Arc::new(RateLimiter::new(0)),
                    max_broadcast_tx_bytes: 1_000_000,
                    db_pool: pool,
                }))
                .service(broadcast_tx),
        )
        .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::post()
                .uri("/tx/raw")
                .set_payload("00")
                .to_request(),
        )
        .await;

        assert_eq!(response.status(), 401);
    }

    #[test]
    fn rapi05_payload_limit_scales_with_broadcast_max() {
        let max_bytes = 1_000_000_usize;
        let payload_limit = max_bytes.saturating_mul(2).max(1024);
        assert_eq!(payload_limit, 2_000_000);
    }

    #[actix_web::test]
    async fn sec04_health_is_exempt_from_rate_limit() {
        let Some(pool) = skip_without_postgres("sec04_health_is_exempt_from_rate_limit") else {
            return;
        };

        let (tx, _rx) = mpsc::channel();
        let app = actix_test::init_service(
            App::new()
                .app_data(web::Data::new(AppState {
                    msg_from_rest_api: tx,
                    api_key: None,
                    rate_limiter: Arc::new(RateLimiter::new(1)),
                    max_broadcast_tx_bytes: 1_000_000,
                    db_pool: pool,
                }))
                .service(health),
        )
        .await;

        for _ in 0..2 {
            let response = actix_test::call_service(
                &app,
                actix_test::TestRequest::get().uri("/health").to_request(),
            )
            .await;
            assert_eq!(response.status(), 200);
        }
    }

    #[actix_web::test]
    async fn bcast05_broadcast_tx_queues_valid_transaction() {
        use chain_gang::{messages::Tx, util::Serializable};

        let Some(pool) = skip_without_postgres("bcast05_broadcast_tx_queues_valid_transaction")
        else {
            return;
        };

        let tx = Tx {
            version: 1,
            inputs: Vec::new(),
            outputs: Vec::new(),
            lock_time: 0,
        };
        let mut bytes = Vec::new();
        tx.write(&mut bytes).expect("serialize tx");
        let hexstr = hex::encode(bytes);

        let (rest_tx, rest_rx) = mpsc::channel();
        let app = actix_test::init_service(
            App::new()
                .app_data(web::Data::new(AppState {
                    msg_from_rest_api: rest_tx,
                    api_key: None,
                    rate_limiter: Arc::new(RateLimiter::new(0)),
                    max_broadcast_tx_bytes: 1_000_000,
                    db_pool: pool,
                }))
                .service(broadcast_tx),
        )
        .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::post()
                .uri("/tx/raw")
                .set_payload(hexstr)
                .to_request(),
        )
        .await;

        assert_eq!(response.status(), 200);
        assert!(matches!(
            rest_rx.try_recv(),
            Ok(RestEventMessage::TxForBroadcast(_))
        ));
    }

    // The 27-byte reproduction, over the real endpoint. If the bounds check is
    // removed this does not fail, it aborts the whole test binary with SIGABRT
    // on an allocation of 281474976710656 bytes.
    #[actix_web::test]
    async fn bcast06_malformed_tx_is_rejected_without_reaching_the_deserialiser() {
        let Some(pool) = skip_without_postgres(
            "bcast06_malformed_tx_is_rejected_without_reaching_the_deserialiser",
        ) else {
            return;
        };

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u32.to_le_bytes()); // version
        bytes.push(0x00); // no inputs
        bytes.push(0x01); // one output
        bytes.extend_from_slice(&0i64.to_le_bytes()); // satoshis
        bytes.push(0xff); // varint: 8-byte length follows
        bytes.extend_from_slice(&(1u64 << 48).to_le_bytes()); // 2^48 byte script
        bytes.extend_from_slice(&0u32.to_le_bytes()); // lock_time
        assert_eq!(bytes.len(), 27);

        // Well under max_broadcast_tx_bytes, so the size cap does not catch it.
        let hexstr = hex::encode(&bytes);

        let (rest_tx, rest_rx) = mpsc::channel();
        let app = actix_test::init_service(
            App::new()
                .app_data(web::Data::new(AppState {
                    msg_from_rest_api: rest_tx,
                    api_key: None,
                    rate_limiter: Arc::new(RateLimiter::new(0)),
                    max_broadcast_tx_bytes: 1_000_000,
                    db_pool: pool,
                }))
                .service(broadcast_tx),
        )
        .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::post()
                .uri("/tx/raw")
                .set_payload(hexstr)
                .to_request(),
        )
        .await;

        assert_eq!(response.status(), 200);
        let body: serde_json::Value = actix_test::read_body_json(response).await;
        assert_eq!(body["status"], "Failed");
        assert!(
            body["detail"]
                .as_str()
                .is_some_and(|d| d.starts_with("Malformed transaction")),
            "expected a malformed-transaction detail, got {:?}",
            body["detail"]
        );
        assert!(
            rest_rx.try_recv().is_err(),
            "a malformed transaction must not be queued for broadcast"
        );
    }
}
