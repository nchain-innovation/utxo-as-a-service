use std::io::Cursor;
use std::sync::mpsc;

use actix_web::{
    delete, get, http::header::ContentType, post, web, HttpResponse, Responder, Result,
};
use serde::Serialize;

use chain_gang::{messages::Tx, util::Serializable};

use crate::config::CollectionConfig;
use crate::uaas::util::decode_hexstr;

// RestEventMessage - used for sending messages from REST API to main event processing loop

#[derive(PartialEq, Clone, Eq)]
pub enum RestEventMessage {
    TxForBroadcast(Tx),
    AddMonitor(CollectionConfig),
    DeleteMonitor(String),
}

// web interface state
pub struct AppState {
    pub msg_from_rest_api: mpsc::Sender<RestEventMessage>,
}

#[derive(Serialize)]
struct BroadcastTxResponse {
    status: String,
    detail: String,
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
async fn broadcast_tx(hexstr: String, data: web::Data<AppState>) -> Result<impl Responder> {
    dbg!(&hexstr);
    // decode the hexstr to tx
    let bytes = match decode_hexstr(&hexstr) {
        Ok(b) => b,
        Err(_) => {
            let response = BroadcastTxResponse {
                status: "Failed".to_string(),
                detail: "Failed to decode hex".to_string(),
            };
            return Ok(web::Json(response));
        }
    };

    let tx = match Tx::read(&mut Cursor::new(&bytes)) {
        Ok(tx) => tx,
        Err(_) => {
            let response = BroadcastTxResponse {
                status: "Failed".to_string(),
                detail: "Failed to convert hex to tx".to_string(),
            };
            return Ok(web::Json(response));
        }
    };

    let hash = tx.hash().encode();

    // Send Tx for broadcast
    data.msg_from_rest_api
        .send(RestEventMessage::TxForBroadcast(tx))
        .unwrap();

    // Return hash as hex_str, if successful
    let response = BroadcastTxResponse {
        status: "Success".to_string(),
        detail: hash,
    };

    Ok(web::Json(response))
}

#[post("/collection/monitor")]
async fn add_monitor(
    monitor: web::Json<CollectionConfig>,
    data: web::Data<AppState>,
) -> Result<impl Responder> {
    let cc = monitor.into_inner();

    data.msg_from_rest_api
        .send(RestEventMessage::AddMonitor(cc))
        .unwrap();

    Ok(HttpResponse::Ok())
}

#[delete("/collection/monitor")]
async fn delete_monitor(monitor_name: String, data: web::Data<AppState>) -> Result<impl Responder> {
    data.msg_from_rest_api
        .send(RestEventMessage::DeleteMonitor(monitor_name))
        .unwrap();

    Ok(HttpResponse::Ok())
}
