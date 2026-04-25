use crate::WsProxyState;
use actix_web::{HttpRequest, Responder, rt, web};
use actix_ws::Message;
use crossbeam::channel;
use futures_util::TryStreamExt;

pub async fn ws(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<WsProxyState>,
) -> actix_web::Result<impl Responder> {
    log::info!("WebSocket connection attempted");

    let (res, mut session, mut stream) = actix_ws::handle(&req, stream)?;

    rt::spawn(async move {
        let uuid = uuid::Uuid::new_v4();
        let (tx, rx) = channel::unbounded::<String>();
        {
            state
                .txs
                .lock()
                .expect("Unable to lock txs")
                .insert(uuid.clone(), tx);
        }
        loop {
            if let Ok(message) = rx.try_recv() {
                log::info!("Sending message");
                session.text(message).await.unwrap();
            }

            if let Ok(Some(msg)) = stream.try_next().await {
                match msg {
                    Message::Text(a) => {
                        log::info!("Text: {}", a);
                    }
                    Message::Close(_) => {
                        let mut txs = state.txs.lock().expect("Unable to lock txs");
                        txs.remove(&uuid);
                        log::info!("Client connection closed. {} clients remain.", txs.len());
                    }
                    Message::Ping(msg) => {
                        if session.pong(&msg).await.is_err() {
                            return;
                        }
                    }
                    _ => {
                        log::info!("{:?}", msg);
                    }
                }
            }
        }
    });

    Ok(res)
}
