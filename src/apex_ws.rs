use crate::WsProxyState;
use axum::extract::ws::Utf8Bytes;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{ConnectInfo, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum_extra::TypedHeader;
use crossbeam::channel;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use uuid::Uuid;

fn string_message(string: String) -> Message {
    Message::Text(Utf8Bytes::from(string))
}

pub(crate) async fn ws_handler(
    State(shared_state): State<Arc<WsProxyState>>,
    ws: WebSocketUpgrade,
    user_agent: Option<TypedHeader<headers::UserAgent>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let user_agent = if let Some(TypedHeader(user_agent)) = user_agent {
        user_agent.to_string()
    } else {
        String::from("Unknown browser")
    };
    log::info!("`{}` at {} connected.", user_agent, addr);
    // finalize the upgrade process by returning upgrade callback.
    // we can customize the callback by sending additional info such as address.
    ws.on_upgrade(move |socket| handle_socket(socket, addr, shared_state.clone()))
}

async fn handle_socket(mut socket: WebSocket, who: SocketAddr, shared_state: Arc<WsProxyState>) {
    if socket
        .send(Message::Ping(axum::body::Bytes::from_static(&[1, 2, 3])))
        .await
        .is_ok()
    {
        log::debug!("Pinged {who}...");
    } else {
        log::warn!("Could not ping {who}!");
        // no Error here since the only thing we can do is to close the connection.
        // If we can not send messages, there is no way to salvage the statemachine anyway.
        return;
    }

    let uuid = Uuid::new_v4();
    let (tx, rx) = channel::unbounded::<String>();
    {
        shared_state.txs.lock().await.insert(uuid.clone(), tx);
    }

    {
        log::info!("Sending new subscriber message to client {}", uuid);
        let socket_state = shared_state.socket_state.lock().await;
        socket
            .send(string_message(
                serde_json::to_string(&*socket_state).unwrap(),
            ))
            .await
            .expect("Unable to send new subscriber message");
        log::info!("New subscriber message: {}", socket_state);
        log::info!("New subscriber message sent to client {}", uuid);
    }

    // By splitting socket we can send and receive at the same time. In this example we will send
    // unsolicited messages to client based on some sort of server's internal event (i.e .timer).
    let (mut sender, mut receiver) = socket.split();

    let send_uuid = uuid.clone();
    // Spawn a task that will push several messages to the client (does not matter what client does)
    let mut send_task = tokio::spawn(async move {
        loop {
            if let Ok(message) = rx.try_recv() {
                log::debug!("Sending message to socket client");
                match sender.send(string_message(message)).await {
                    Ok(_) => {}
                    Err(e) => {
                        log::error!(
                            "Unable to send message from scoreboard to client {}: {}",
                            send_uuid,
                            e
                        );
                        break;
                    }
                }
            } else {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            }
        }
    });

    let recv_uuid = uuid.clone();
    // This second task will receive messages from client and print them on server console
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            // print message and break if instructed to do so
            log::info!(
                r#"Message received from client {}: "{}""#,
                recv_uuid,
                msg.to_text().unwrap()
            );
        }
    });

    // If any one of the tasks exit, abort the other.
    tokio::select! {
        rv_a = (&mut send_task) => {
            match rv_a {
                Ok(_) => log::debug!("Receiver {} closed", uuid),
                Err(a) => log::warn!("Error sending messages {a:?}")
            }
            shared_state.txs.lock().await.remove(&uuid);
            recv_task.abort();
        },
        rv_b = (&mut recv_task) => {
            match rv_b {
                Ok(_) => log::debug!("Sender {} closed", uuid),
                Err(b) => log::warn!("Error receiving messages {b:?}")
            }
            shared_state.txs.lock().await.remove(&uuid);
            send_task.abort();
        }
    }

    // returning from the handler closes the websocket connection
    log::debug!("Websocket context {who} destroyed");
}
