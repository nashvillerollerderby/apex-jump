use axum::Router;
use axum::routing::{MethodRouter, any};
use crossbeam::channel;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::args::Args;

cfg_if::cfg_if! {
    if #[cfg(feature = "static-files")] {
        use axum::http::header::CONTENT_TYPE;
        use axum::http::{Response, StatusCode};
        use axum::routing::get;
        use std::fs;
        use std::path::{Path, PathBuf};
        use tower_http::services::ServeDir;
    }
}

/// Initializes logging from `./log4rs.yaml`, or from the default `log4rs.yaml` in the
/// [`apex-jump` repository](https://github.com/nashvillerollerderby/apex-jump/blob/main/log4rs.yaml).
pub fn init_logging() {
    match log4rs::init_file("log4rs.yaml", Default::default()) {
        Ok(_) => {
            log::info!("Logging has been initialized.");
        }
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

/// The proxy's state object, storing Arc'd Mutexes of mutable values.
#[derive(Clone)]
pub struct WsProxyState {
    /// The full state of all WebSocket messages received from the CRG Scoreboard.
    #[allow(unused)]
    pub socket_state: Arc<Mutex<Value>>,
    pub(crate) txs: Arc<Mutex<HashMap<Uuid, channel::Sender<String>>>>,
    pub(crate) crg_ws_reconnect_rate_s: u64,
    pub(crate) stop: Arc<Mutex<bool>>,
    pub(crate) registration_paths: Option<Vec<String>>,
    /// An empty Map to be used by anything external to `apex-jump`.
    #[allow(unused)]
    pub ext: Arc<Mutex<Map<String, Value>>>,
}

unsafe impl Send for WsProxyState {}
unsafe impl Sync for WsProxyState {}

impl WsProxyState {
    fn new(crg_ws_reconnect_rate_s: u64, registration_paths: Option<Vec<String>>) -> Self {
        WsProxyState {
            socket_state: Default::default(),
            txs: Default::default(),
            stop: Arc::new(Mutex::new(false)),
            registration_paths,
            crg_ws_reconnect_rate_s,
            ext: Default::default(),
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
    services: HashMap<String, Router<Arc<WsProxyState>>>,
    routes: HashMap<String, MethodRouter<Arc<WsProxyState>>>,
}

const LOG4RS_DEFAULT_CONFIG: &str = include_str!("../log4rs.yaml");

async fn shutdown(state: Arc<WsProxyState>) {
    let ctrl_c = tokio::signal::ctrl_c();
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("failed to install signal handler");
    let mut terminate = Box::pin(terminate.recv());

    tokio::select! {
        _ = ctrl_c => {
            let mut stop = state.stop.lock().await;
            *stop = true;
        },
        _ = &mut terminate => {
            let mut stop = state.stop.lock().await;
            *stop = true;
        },
    }
}

impl WsProxy {
    /// Creates an instance of the WsProxyBuilder, which allows one to use the builder pattern to
    /// configure a WsProxy instance.
    pub fn builder() -> WsProxyBuilder {
        WsProxyBuilder {
            crg_ws_reconnect_rate_s: 5,
            ..Default::default()
        }
    }

    /// Start the `apex-jump` WebSocket proxy server.
    ///
    /// **This will block the thread on which it is run until the server is stopped.**
    ///
    /// # Example
    /// ```rust
    /// use apex_jump::WsProxy;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     WsProxy::builder()
    ///         // configure your WsProxy here
    ///         .build()
    ///         .start()
    ///         .await
    /// }
    /// ```
    pub async fn start(&self) -> std::io::Result<()> {
        let ip = self.ip.clone().unwrap_or("127.0.0.1".to_string());
        let port = self.port.clone().unwrap_or(8001);

        let shared_state = self.state.clone();

        crate::crg_ws::init(self.crg.clone(), self.state.clone()).await;

        #[allow(unused_mut)]
        let mut app = Router::new()
            .layer(ServiceBuilder::new().layer(TraceLayer::new_for_http()))
            .route("/WS/", any(crate::apex_ws::ws_handler));

        #[cfg(feature = "static-files")]
        if let Some(ref dir) = self.static_dir {
            let file_service = ServeDir::new(dir.clone());
            let router = handle_directories_with_router(dir).fallback_service(file_service);
            app = app.fallback_service(router);
        }

        for (path, service) in self.services.clone() {
            app = app.nest(&path, service);
        }

        for (path, route) in &self.routes {
            app = app.route(path, route.clone());
        }

        let app = app.with_state(shared_state.clone());

        log::info!("Starting server on {}:{}", ip, port);
        let listener = tokio::net::TcpListener::bind(format!("{}:{}", ip, port)).await?;
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown(shared_state))
        .await
    }

    /// Open a new subscriber to the internal CRG WebSocket state in Rust.
    ///
    /// Returns a tuple containing the UUID of the subscriber and the receiver channel.
    ///
    /// # Example
    ///
    /// ```rust
    /// let (uuid, subscriber) = proxy.new_subscriber().await;
    /// while let Some(Ok(msg)) = receiver.next().await {
    ///     log::info!(
    ///         r#"Message received: "{}""#,
    ///.        msg.to_text().unwrap()
    ///     );
    /// }
    /// proxy.close_subscriber(uuid).await;
    /// ```
    #[allow(unused)]
    pub async fn new_subscriber(&self) -> (Uuid, channel::Receiver<String>) {
        let (tx, rx) = channel::unbounded::<String>();
        let uuid = Uuid::new_v4();
        {
            let socket_state = self.state.socket_state.lock().await;
            tx.send(serde_json::to_string(&*socket_state).unwrap())
                .unwrap();
        }
        {
            let mut txs = self.state.txs.lock().await;
            txs.insert(uuid, tx);
        }
        (uuid, rx)
    }

    /// Closes a Rust-based subscriber to the internal CRG WebSocket state.
    #[allow(unused)]
    pub async fn close_subscriber(&self, uuid: Uuid) {
        let mut txs = self.state.txs.lock().await;
        txs.remove(&uuid);
    }
}

/// A structure that allows one to use the builder pattern to configure an instance of the WsProxy.
///
/// # Example
/// ```rust
/// let proxy = WsProxy::builder()
///     .for_crg("localhost", 8000)
///     .build();
/// proxy.start().await;
/// ```
#[derive(Default)]
pub struct WsProxyBuilder {
    crg_host: Option<String>,
    crg_port: Option<u16>,
    crg_ws_reconnect_rate_s: u64,
    port: Option<u16>,
    #[cfg(feature = "static-files")]
    static_dir: Option<String>,
    registration_paths: Option<Vec<String>>,
    services: HashMap<String, Router<Arc<WsProxyState>>>,
    routes: HashMap<String, MethodRouter<Arc<WsProxyState>>>,
}

impl WsProxyBuilder {
    /// From the currently configured state of the WsProxyBuilder, instantiate a WsProxy with the
    /// same configuration.
    pub fn build(self) -> WsProxy {
        let crg_host = self
            .crg_host
            .expect("Unable to start server without CRG host address");
        let crg_port = self
            .crg_port
            .expect("Unable to start server without CRG host port");
        WsProxy {
            state: Arc::new(WsProxyState::new(
                self.crg_ws_reconnect_rate_s,
                self.registration_paths,
            )),
            ip: None,
            port: self.port,
            #[cfg(feature = "static-files")]
            static_dir: self.static_dir,
            crg: (crg_host, crg_port),
            services: self.services,
            routes: self.routes,
        }
    }

    /// Set the host address and port on which the CRG Scoreboard is running.
    pub fn for_crg(mut self, host: &str, port: u16) -> Self {
        self.crg_host = Some(host.into());
        self.crg_port = Some(port);
        self
    }

    /// Set the port to which the WsProxy should bind.
    pub fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// Set the registration paths for the CRG Scoreboard WebSocket.
    ///
    /// An example of a registration path would be "ScoreBoard.Game(*)".
    ///
    /// # Example
    /// ```rust
    /// let builder = WsProxy::builder()
    ///     .with_registration_paths(vec![
    ///         "ScoreBoard.Game(*)".to_string(),
    ///         "ScoreBoard.CurrentGame".to_string()
    ///     ]);
    /// ```
    pub fn with_registration_paths(mut self, registration_paths: Vec<String>) -> Self {
        if registration_paths.len() > 0 {
            self.registration_paths = Some(registration_paths);
        }
        self
    }

    /// Set a folder for the proxy server to host as static files.
    #[cfg(feature = "static-files")]
    pub fn with_static(mut self, dir: String) -> Self {
        self.static_dir = Some(dir);
        self
    }

    /// Set all the args in one command using crate::Args.
    pub fn with_config(mut self, config: Args) -> Self {
        self.crg_port = Some(config.crg_port);
        self.crg_host = Some(config.crg_host);
        self.port = Some(config.port);
        self.crg_ws_reconnect_rate_s = config.crg_ws_reconnect_s;
        self.registration_paths = if config.registration_paths.len() > 0 {
            Some(config.registration_paths)
        } else {
            None
        };
        self
    }

    /// Set an axum::routing::MethodRouter at a path.
    ///
    /// Routes are applied the order in which they are set, after `/WS/`. Routes can also access the
    /// Axum shared state.
    ///
    /// # Examples
    /// ## Basic Example
    /// ```rust
    /// use apex_jump::WsProxy;
    /// use axum::response::IntoResponse;
    ///
    /// fn hello_world() -> impl IntoResponse {
    ///     "Hello World!"
    /// }
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let builder = WsProxy::builder()
    ///         .with_route("/map", get(hello_world));
    /// }
    /// ```
    ///
    /// ## Using shared WsProxyState
    /// ```rust
    /// use apex_jump::WsProxy;
    /// use axum::response::IntoResponse;
    ///
    /// async fn ext_map(State(state): State<Arc<WsProxyState>>) -> impl IntoResponse {
    ///     serde_json::to_string(&Value::Object(state.ext.lock().await.clone())).unwrap()
    /// }
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let builder = WsProxy::builder()
    ///         .with_route("/map", get(ext_map));
    /// }
    /// ```
    #[allow(unused)]
    pub fn with_route(mut self, path: &str, route: MethodRouter<Arc<WsProxyState>>) -> Self {
        self.routes.insert(path.to_string(), route);
        self
    }

    /// Set an axum::routing::Router at a path.
    ///
    /// Services are applied the order in which they are set, after `/WS/` and **all** routes.
    ///
    /// # Example
    /// ```rust
    /// use apex_jump::WsProxy;
    /// use axum::response::IntoResponse;
    /// use axum::routing::Router;
    ///
    /// fn hello_world() -> impl IntoResponse {
    ///     "Hello World!"
    /// }
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let router = Router::new().route("/hello", get(hello_world));
    ///     let builder = WsProxy::builder()
    ///         .with_service("/svc", router);
    /// }
    /// ```
    #[allow(unused)]
    pub fn with_service(mut self, path: &str, service: Router<Arc<WsProxyState>>) -> Self {
        self.services.insert(path.to_string(), service);
        self
    }
}
