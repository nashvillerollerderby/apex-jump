use apex_jump::{WsProxy, WsProxyState};
use axum::Router;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::get;
use serde_json::{Value, json};
use std::sync::Arc;

async fn hello_world() -> impl IntoResponse {
    "Hello World!"
}

// The routes can access the state on the webserver, allowing you full access to the `ext` map and
// `socket_state` serde_json::Value (which is a map) to do with what you please. It is strongly
// advised _not_ to write over the value in `socket_state`, but it can certainly be helpful to read
// from that value as you need.
async fn ext_map(State(state): State<Arc<WsProxyState>>) -> impl IntoResponse {
    serde_json::to_string(&Value::Object(state.ext.lock().await.clone())).unwrap()
}

async fn set_key(
    State(state): State<Arc<WsProxyState>>,
    Path((key, value)): Path<(String, Value)>,
) -> impl IntoResponse {
    let mut lock = state.ext.lock().await;
    lock.insert(key, value);
    "ok"
}

async fn svc_ext_map(State(state): State<Arc<WsProxyState>>) -> impl IntoResponse {
    let mut lock = state.ext.lock().await;
    let svc_state = match lock.get("svc") {
        Some(state) => state,
        None => &lock.insert("svc".to_string(), json!({})).unwrap(),
    };
    serde_json::to_string(&svc_state.clone()).unwrap()
}

async fn svc_set_key(
    State(state): State<Arc<WsProxyState>>,
    Path((key, value)): Path<(String, Value)>,
) -> impl IntoResponse {
    let mut lock = state.ext.lock().await;
    let svc_state: &mut Value = match lock.get_mut("svc") {
        Some(state) => state,
        None => &mut lock.insert("svc".to_string(), json!({})).unwrap(),
    };
    let svc_state: &mut serde_json::Map<String, Value> = svc_state.as_object_mut().unwrap();
    svc_state.insert(key, value);
    "ok"
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Define a router that accepts the state, and you can create a service that uses the internal
    // apex-jump state.
    let service = Router::new()
        .route("/ext-map", get(svc_ext_map))
        .route("/set/{key}/{value}", get(svc_set_key));

    WsProxy::builder()
        // Set the CRG host/port
        .for_crg("localhost", 8000)
        // Add your service
        .with_service("/svc", service)
        // Add any explicit routes you need
        .with_route("/hello-world", get(hello_world))
        .with_route("/ext-map", get(ext_map))
        .with_route("/set/{key}/{value}", get(set_key))
        // Build and start the webserver
        .build()
        .start()
        .await
}
