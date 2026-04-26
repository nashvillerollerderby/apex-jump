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

    /// The port on which the apex-jump server should be started
    #[arg(short, long, default_value_t = 8001)]
    port: u16,

    /// The number of workers with which the apex-jump server should launch
    #[arg(short, long)]
    worker_count: Option<usize>,

    #[cfg(feature = "static-files")]
    #[arg(long)]
    /// File mounts formatted as "<server_path>;<absolute_os_path_to_files>[;<index_file>]"
    files: Vec<String>,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    init_logging();

    let mut proxy = WsProxy::builder();

    #[cfg(feature = "static-files")]
    for static_files in args.files {
        let s = static_files.split(';').collect::<Vec<&str>>();
        match s.len() {
            3 => {
                proxy = proxy.with_static(s[0], s[1], Some(s[2]));
            }
            2 => {
                proxy = proxy.with_static(s[0], s[1], None);
            }
            _ => {
                log::error!("Files argument should be 2 or 3 segments, separated by a semicolon.");
                std::process::exit(1);
            }
        }
    }

    proxy = proxy.crg(&args.crg_host, args.crg_port).port(args.port);
    if let Some(worker_count) = args.worker_count {
        proxy = proxy.worker_count(worker_count);
    }

    proxy.build().start().await
}
