use axum::Router;
use axum::routing::any;
use crossbeam::channel;
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

#[cfg(feature = "static-files")]
use axum::http::header::CONTENT_TYPE;
#[cfg(feature = "static-files")]
use axum::http::{Response, StatusCode};
#[cfg(feature = "static-files")]
use axum::routing::get;
#[cfg(feature = "static-files")]
use std::fs;
#[cfg(feature = "static-files")]
use std::path::{Path, PathBuf};
#[cfg(feature = "static-files")]
use tower_http::services::ServeDir;

pub fn init_logging() {
    match log4rs::init_file("log4rs.yaml", Default::default()) {
        Ok(_) => {}
        Err(_) => {
            let config =
                serde_yaml_ng::from_str::<log4rs::config::RawConfig>(LOG4RS_DEFAULT_CONFIG)
                    .unwrap();
            log4rs::init_raw_config(config).expect("Unable to initialize with default config.");
            log::info!(
                "No log4rs.yaml file found in working directory. Using default config (stdout)."
            );
        }
    }
}

#[cfg(feature = "static-files")]
fn directory_fallback(path: PathBuf) -> Response<String> {
    let files = fs::read_dir(path).unwrap();
    let mut values = Vec::new();
    for file in files {
        match file {
            Ok(file) => values.push(Value::from(file.file_name().to_str().unwrap())),
            Err(e) => {
                log::error!("{}", e);
            }
        }
    }
    let value = Value::Array(values);
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/json")
        .body(value.to_string())
        .unwrap()
}

#[cfg(feature = "static-files")]
fn handle_directory(path: PathBuf) -> Router {
    let mut router = Router::new();
    let dir = fs::read_dir(path.clone()).unwrap();
    let mut has_index = false;
    for file in dir {
        match file {
            Ok(file) => {
                if file.metadata().unwrap().is_dir() {
                    router = router.nest_service(
                        &format!("/{}", file.file_name().to_str().unwrap()),
                        handle_directory(file.path()),
                    );
                } else {
                    if file.file_name() == "index.html" {
                        log::debug!("{:?} has index", path);
                        has_index = true;
                        continue;
                    }
                }
            }
            Err(e) => {
                log::error!("{}", e);
            }
        }
    }
    if !has_index {
        router = router
            .route("/", get(directory_fallback(path.clone())))
            .fallback_service(ServeDir::new(path));
    } else {
        router = router.fallback_service(ServeDir::new(path));
    }
    router
}

#[cfg(feature = "static-files")]
fn handle_directories_with_router(dir: &String) -> Router {
    let mut router = Router::new();

    let path = Path::new(dir);
    if fs::metadata(path).unwrap().is_dir() {
        router = handle_directory(path.to_path_buf());
    }

    router
}

pub(crate) struct WsProxyState {
    #[allow(unused)]
    pub(crate) socket_state: Arc<Mutex<Value>>,
    pub(crate) txs: Arc<Mutex<HashMap<uuid::Uuid, channel::Sender<String>>>>,
    pub(crate) crg_ws_reconnect_rate_s: u64,
}

unsafe impl Send for WsProxyState {}
unsafe impl Sync for WsProxyState {}

impl WsProxyState {
    fn new(crg_ws_reconnect_rate_s: u64) -> Self {
        WsProxyState {
            socket_state: Default::default(),
            txs: Default::default(),
            crg_ws_reconnect_rate_s,
        }
    }
}

pub struct WsProxy {
    state: Arc<WsProxyState>,
    ip: Option<String>,
    port: Option<u16>,
    crg: (String, u16),
    #[cfg(feature = "static-files")]
    static_dir: Option<String>,
}

const LOG4RS_DEFAULT_CONFIG: &str = include_str!("../log4rs.yaml");

impl WsProxy {
    pub fn builder() -> WsProxyBuilder {
        WsProxyBuilder {
            crg_ws_reconnect_rate_s: 5,
            ..Default::default()
        }
    }

    pub async fn start(&self) -> std::io::Result<()> {
        let ip = self.ip.clone().unwrap_or("127.0.0.1".to_string());
        let port = self.port.clone().unwrap_or(8001);

        let shared_state = self.state.clone();

        crate::crg_ws::init(self.crg.clone(), self.state.clone()).await;

        #[allow(unused_mut)]
        let mut app = Router::new()
            .route("/WS/", any(crate::apex_ws::ws_handler))
            .layer(ServiceBuilder::new().layer(TraceLayer::new_for_http()))
            .with_state(shared_state);

        #[cfg(feature = "static-files")]
        if let Some(ref dir) = self.static_dir {
            let file_service = ServeDir::new(dir.clone());
            let router = handle_directories_with_router(dir).fallback_service(file_service);
            app = app.fallback_service(router);
        }

        log::info!("Starting server on {}:{}", ip, port);
        let listener = tokio::net::TcpListener::bind(format!("{}:{}", ip, port)).await?;
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
    }
}

#[derive(Default)]
pub struct WsProxyBuilder {
    crg_host: Option<String>,
    crg_port: Option<u16>,
    crg_ws_reconnect_rate_s: u64,
    port: Option<u16>,
    #[cfg(feature = "static-files")]
    static_dir: Option<String>,
}

impl WsProxyBuilder {
    pub fn build(self) -> WsProxy {
        let crg_host = self
            .crg_host
            .expect("Unable to start server without CRG host address");
        let crg_port = self
            .crg_port
            .expect("Unable to start server without CRG host port");
        WsProxy {
            state: Arc::new(WsProxyState::new(self.crg_ws_reconnect_rate_s)),
            ip: None,
            port: self.port,
            #[cfg(feature = "static-files")]
            static_dir: self.static_dir,
            crg: (crg_host, crg_port),
        }
    }

    pub fn crg(mut self, host: &str, port: u16) -> Self {
        self.crg_host = Some(host.into());
        self.crg_port = Some(port);
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    #[cfg(feature = "static-files")]
    pub fn with_static(mut self, dir: String) -> Self {
        self.static_dir = Some(dir);
        self
    }
}
