use std::io::Cursor;
use std::sync::mpsc;

use actix_web::{
    delete, get, http::header::ContentType, post, web, HttpRequest, HttpResponse, Responder, Result,
};
use mysql::{prelude::*, Pool};
use serde::Serialize;

use chain_gang::{messages::Tx, util::Serializable};

use crate::config::CollectionConfig;
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
    pub db_pool: Pool,
}

const API_KEY_HEADER: &str = "X-API-Key";

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

fn check_database(pool: &Pool) -> Result<(), String> {
    let mut conn = pool.get_conn().map_err(|err| err.to_string())?;
    conn.query_first::<u8, _>("SELECT 1")
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "database health check returned no rows".to_string())?;
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
async fn version(_data: web::Data<AppState>) -> impl Responder {
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
    if let Some(response) = authorize(&req, &data.api_key) {
        return Ok(response);
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
    if let Some(response) = authorize(&req, &data.api_key) {
        return Ok(response);
    }

    log::info!("delete_monitor '{}'", &monitor_name);

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

    fn mysql_test_url() -> Option<String> {
        std::env::var("UAAS_TEST_MYSQL_URL").ok()
    }

    fn live_db_pool() -> Option<Pool> {
        let url = mysql_test_url()?;
        Some(Pool::new(url.as_str()).expect("failed to connect to test database"))
    }

    fn invalid_credentials_pool() -> Option<Pool> {
        let url = mysql_test_url()?;
        let bad_url = url.replacen("maas:maas-password", "maas:not-the-password", 1);
        Pool::new(bad_url.as_str()).ok()
    }

    fn skip_without_mysql(test_name: &str) -> Option<Pool> {
        live_db_pool().or_else(|| {
            eprintln!("skipping {test_name}: UAAS_TEST_MYSQL_URL not set");
            None
        })
    }

    mod database_checks {
        use super::*;

        #[test]
        fn live_mysql_passes_health_check() {
            let Some(pool) = skip_without_mysql("live_mysql_passes_health_check") else {
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
            db_pool,
        })
    }

    #[actix_web::test]
    async fn health_returns_ok_with_database() {
        let Some(pool) = skip_without_mysql("health_returns_ok_with_database") else {
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
        let Some(pool) = skip_without_mysql("version_returns_package_version") else {
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
        let Some(pool) = skip_without_mysql("health_does_not_require_api_key") else {
            return;
        };

        let (tx, _rx) = mpsc::channel();
        let app = actix_test::init_service(
            App::new()
                .app_data(web::Data::new(AppState {
                    msg_from_rest_api: tx,
                    api_key: Some("secret-key".to_string()),
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
        let Some(pool) = skip_without_mysql("broadcast_tx_requires_api_key_when_configured") else {
            return;
        };

        let (tx, _rx) = mpsc::channel();
        let app = actix_test::init_service(
            App::new()
                .app_data(web::Data::new(AppState {
                    msg_from_rest_api: tx,
                    api_key: Some("secret-key".to_string()),
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
}
