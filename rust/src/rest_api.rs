use std::io::Cursor;
use std::sync::Mutex;

use actix_web::{post, web, Responder, Result};
use serde::Serialize;

use sv::messages::Tx;
use sv::util::Serializable;

use crate::uaas::util::decode_hexstr;

// web interface state
#[derive(Default)]
pub struct AppState {
    pub txs_for_broadcast: Mutex<Vec<Tx>>,
}

#[derive(Serialize)]
struct BroadcastTxResponse {
    status: String,
    detail: String,
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

    // Queue Tx to send - append to txs_for_broadcast
    let mut txs_for_broadcast = data.txs_for_broadcast.lock().unwrap();
    txs_for_broadcast.push(tx);

    // Return hash as hex_str, if successful
    let response = BroadcastTxResponse {
        status: "Success".to_string(),
        detail: hash,
    };

    Ok(web::Json(response))
}
