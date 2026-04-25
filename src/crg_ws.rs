use crate::WsProxyState;
use std::sync::Arc;
use std::thread::JoinHandle;
use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, connect};

pub(crate) fn init(crg: (String, u16), state: Arc<WsProxyState>) -> JoinHandle<()> {
    let state = state.clone();
    let request = (&format!("ws://{}:{}/WS", crg.0, crg.1))
        .into_client_request()
        .unwrap();
    let handle = std::thread::spawn(move || {
        let state = state.clone();
        let (mut stream, _response) = connect(request.clone()).unwrap();
        log::info!("CRG WS Connected");
        let json = serde_json::json!({
            "action": "Register",
            "paths": [
                "ScoreBoard.Game(*)",
            ]
        });

        stream.send(Message::text(json.to_string())).unwrap();
        loop {
            match stream.read() {
                Ok(message) => match message {
                    msg @ Message::Text(_) => {
                        log::info!("Message received from CRG");
                        let lock = state.txs.lock().expect("Unable to lock shared state");
                        lock.iter().for_each(|(_, tx)| {
                            tx.send(msg.clone().into_text().unwrap().parse().unwrap())
                                .unwrap();
                        });
                    }
                    _msg @ Message::Close(_) => {
                        stream = connect(request.clone()).unwrap().0;
                    }
                    _ => {}
                },
                Err(e) => {
                    log::error!("{}", e);
                }
            }
        }
    });
    handle
}
