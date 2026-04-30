pub(crate) mod apex_ws;
pub(crate) mod crg_ws;
mod proxy_server;

use clap::Parser;
use proxy_server::*;

/// Runs a WebSocket server that proxies the WebSocket data from the CRG Scoreboard to additional clients.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The host server for the CRG Scoreboard instance
    #[arg(long, default_value = "localhost")]
    crg_host: String,

    /// The host port for the CRG Scoreboard instance
    #[arg(long, default_value_t = 8000)]
    crg_port: u16,

    /// The number of seconds to wait between attempts to reconnect to the CRG Scoreboard WebSocket
    #[arg(short = 'r', long = "reconnect-delay-s", default_value_t = 5)]
    crg_ws_reconnect_s: u64,

    /// The port on which the apex-jump server should be started
    #[arg(short, long, default_value_t = 8001)]
    port: u16,

    /// File mount formatted as "<path_to_files>"
    #[cfg(feature = "static-files")]
    #[arg(short, long)]
    files: Option<String>,
}

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

    #[cfg(feature = "static-files")]
    if let Some(files) = args.files {
        proxy = proxy.with_static(files);
    }

    proxy
        .crg(&args.crg_host, args.crg_port)
        .port(args.port)
        .build()
        .start()
        .await
}
