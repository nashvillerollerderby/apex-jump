use actix_web::dev::Service;
use actix_web::web::ServiceConfig;
use actix_web::{App, Error, HttpRequest, HttpResponse, HttpServer, middleware, rt, web};
use actix_ws::AggregatedMessage;
use crossbeam::channel;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio_stream::StreamExt as _;

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

pub struct WsProxyState {
    pub(crate) socket_state: Mutex<serde_json::Map<String, serde_json::Value>>,
    pub(crate) txs: Mutex<HashMap<uuid::Uuid, channel::Sender<String>>>,
}

unsafe impl Send for WsProxyState {}
unsafe impl Sync for WsProxyState {}

impl WsProxyState {
    fn new() -> Self {
        WsProxyState {
            socket_state: Default::default(),
            txs: Default::default(),
        }
    }
}

pub struct WsProxy {
    state: Arc<WsProxyState>,
    ip: Option<String>,
    port: Option<u16>,
    #[cfg(feature = "static-files")]
    static_dirs: HashMap<String, (String, Option<String>)>,
    crg: (String, u16),
}

#[cfg(feature = "static-files")]
fn configure_static_dirs(
    app: &mut ServiceConfig,
    static_dirs: &Vec<(String, String, Option<String>)>,
) {
    for (mount_path, dir, with_index) in static_dirs {
        if let Some(index) = with_index {
            app.service(
                actix_files::Files::new(mount_path, dir)
                    .index_file(index)
                    .show_files_listing(),
            );
        } else {
            app.service(actix_files::Files::new(mount_path, dir).show_files_listing());
        }
    }
}

const LOG4RS_DEFAULT_CONFIG: &str = include_str!("../log4rs.yaml");

impl WsProxy {
    pub fn builder() -> WsProxyBuilder {
        WsProxyBuilder {
            ..Default::default()
        }
    }

    #[cfg(feature = "static-files")]
    fn static_dirs(&self) -> Vec<(String, String, Option<String>)> {
        let mut static_dirs = Vec::new();
        for (mount_path, (dir, with_index)) in self.static_dirs.clone() {
            static_dirs.push((mount_path, dir, with_index));
        }
        static_dirs
    }

    pub async fn start(&self) -> std::io::Result<()> {
        let ip = self.ip.clone().unwrap_or("127.0.0.1".to_string());
        let port = self.port.clone().unwrap_or(8001);

        #[cfg(feature = "static-files")]
        let static_dirs = self.static_dirs();

        let shared_state = web::Data::new(self.state.clone());

        crate::crg_ws::init(self.crg.clone(), self.state.clone());

        log::info!("Starting server on {}:{}", ip, port);
        HttpServer::new(move || {
            App::new()
                .wrap(middleware::NormalizePath::trim())
                .wrap(middleware::Logger::default())
                .app_data(web::Data::clone(&shared_state))
                .service(web::resource("/ws").route(web::get().to(crate::apex_ws::ws)))
                .configure(|app| {
                    #[cfg(feature = "static-files")]
                    configure_static_dirs(app, &static_dirs)
                })
        })
        .workers(4)
        .bind((ip, port))?
        .run()
        .await
    }
}

#[derive(Default)]
pub struct WsProxyBuilder {
    crg_host: Option<String>,
    crg_port: Option<u16>,
    #[cfg(feature = "static-files")]
    static_dirs: HashMap<String, (String, Option<String>)>,
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
            state: Arc::new(WsProxyState::new()),
            ip: None,
            port: None,
            #[cfg(feature = "static-files")]
            static_dirs: self.static_dirs,
            crg: (crg_host, crg_port),
        }
    }

    pub fn crg(mut self, host: &str, port: u16) -> Self {
        self.crg_host = Some(host.into());
        self.crg_port = Some(port);
        self
    }

    #[cfg(feature = "static-files")]
    pub fn with_static(mut self, mount_path: &str, dir: &str, with_index: Option<&str>) -> Self {
        self.static_dirs
            .entry(mount_path.into())
            .and_modify(|value| {
                value.0 = dir.to_string();
                value.1 = match with_index {
                    Some(index) => Some(index.to_string()),
                    None => None,
                }
            })
            .or_insert_with(|| {
                (
                    dir.to_string(),
                    match with_index {
                        Some(index) => Some(index.to_string()),
                        None => None,
                    },
                )
            });

        self
    }
}
