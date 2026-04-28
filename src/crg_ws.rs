use crate::WsProxyState;
use axum::body::Bytes;
use futures_util::SinkExt;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};
use tokio_stream::StreamExt;
use tokio_tungstenite::connect_async;
use tungstenite::Message;
use tungstenite::client::IntoClientRequest;

fn deep_merge(left: &mut Value, right: Value) {
    match (left, right) {
        (Value::Object(left), Value::Object(right)) => {
            for (key, value) in right {
                deep_merge(left.entry(key).or_insert(Value::Null), value);
            }
        }
        (left, right) => *left = right,
    }
}

static PING_BYTES: &[u8] = "".as_bytes();

pub(crate) async fn init(crg: (String, u16), state: Arc<WsProxyState>) -> JoinHandle<()> {
    let state = state.clone();
    let request = (&format!("ws://{}:{}/WS", crg.0, crg.1))
        .into_client_request()
        .unwrap();
    let handle = tokio::spawn(async move {
        'socket: loop {
            let state = state.clone();
            log::info!("Connecting to CRG Scoreboard WS");
            let (mut stream, _response) = match connect_async(request.clone()).await {
                Ok(s) => s,
                Err(e) => {
                    log::error!(
                        "Unable to connect to CRG Scoreboard WS: {}; waiting {}s before trying again",
                        e,
                        state.crg_ws_reconnect_rate_s
                    );
                    sleep(Duration::from_secs(state.crg_ws_reconnect_rate_s)).await;
                    continue 'socket;
                }
            };
            log::info!("CRG Scoreboard WS Connected");
            let json = json!({
                "action": "Register",
                "paths": [
                    "ScoreBoard.Game(*)",
                    "ScoreBoard.CurrentGame",
                    "ScoreBoard.Settings",
                    "ScoreBoard.Version",
                ]
            });

            stream.send(Message::text(json.to_string())).await.unwrap();
            log::info!("Registered for updates");

            log::info!("Looping for CRG Scoreboard WS");
            'main: loop {
                match timeout(Duration::from_secs(10), StreamExt::try_next(&mut stream)).await {
                    Ok(Ok(Some(message))) => match message {
                        msg @ Message::Text(_) => {
                            log::info!("Message received from CRG: {}", msg);
                            {
                                let mut lock = state.socket_state.lock().await;
                                let map = serde_json::from_str::<Value>(&msg.to_string()).unwrap();
                                deep_merge(&mut *lock, map);
                            }

                            let lock = state.txs.lock().await;
                            lock.iter().for_each(|(_, tx)| {
                                match tx.send(msg.clone().into_text().unwrap().parse().unwrap()) {
                                    Ok(_) => {}
                                    Err(e) => {
                                        log::error!("{}", e);
                                    }
                                }
                            });
                        }
                        Message::Close(_) => {
                            log::error!(
                                "Connection to CRG Scoreboard has closed. Attempting to reconnect in {}s.",
                                state.crg_ws_reconnect_rate_s
                            );
                            break 'main;
                        }
                        Message::Ping(bytes) => {
                            log::debug!("Ping received, sending Pong");
                            stream.send(Message::Pong(bytes)).await.unwrap();
                        }
                        _ => {}
                    },
                    Ok(Ok(None)) => {
                        log::error!(
                            "Connection to CRG Scoreboard has closed. Attempting to reconnect in {}s.",
                            state.crg_ws_reconnect_rate_s
                        );
                        break 'main;
                    }
                    Ok(Err(e)) => {
                        log::error!("{}", e);
                    }
                    Err(elapsed) => {
                        log::debug!("{}, sending ping", elapsed);
                        stream
                            .send(Message::Ping(Bytes::from_static(PING_BYTES)))
                            .await
                            .unwrap();
                    }
                }
            }
            sleep(Duration::from_secs(5)).await;
        }
    });
    handle
}
