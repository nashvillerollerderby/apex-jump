pub(crate) mod apex_ws;
mod args;
pub(crate) mod crg_ws;
mod proxy_server;

use crate::args::Args;
use clap::Parser;
use proxy_server::*;
use std::path::PathBuf;
use std::str::FromStr;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    init_logging();

    #[cfg(not(feature = "static-files"))]
    log::info!("Running apex-jump");
    #[cfg(feature = "static-files")]
    log::info!("Running apex-jump with integrated fileserver");

    #[allow(unused_mut)]
    let mut proxy = WsProxy::builder();

    if let Some(config_file_path) = args.config_file {
        let path = PathBuf::from_str(&config_file_path).unwrap();
        let file_content = std::fs::read_to_string(path)?;

        let args = serde_yaml_ng::from_str::<Args>(&file_content).unwrap();
        proxy = proxy.with_config(args);
    } else {
        #[cfg(feature = "static-files")]
        if let Some(files) = args.files {
            proxy = proxy.with_static(files);
        }
        proxy = proxy
            .for_crg(&args.crg_host, args.crg_port)
            .port(args.port)
            .with_registration_paths(args.registration_paths);
    }

    proxy.build().start().await
}
