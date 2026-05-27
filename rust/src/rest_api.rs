use std::io::Cursor;
use std::sync::mpsc;

use actix_web::{
    delete, get, http::header::ContentType, post, web, HttpRequest, HttpResponse, Responder, Result,
};
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
    pub max_broadcast_tx_bytes: usize,
}

fn tx_hex_exceeds_limit(hex_len: usize, max_tx_bytes: usize) -> bool {
    hex_len / 2 > max_tx_bytes
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
    version: &'static str,
}

#[get("/health")]
async fn health() -> impl Responder {
    web::Json(HealthResponse {
        status: "ok",
        service: "uaas-service",
        version: env!("CARGO_PKG_VERSION"),
    })
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
