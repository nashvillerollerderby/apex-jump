pub(crate) mod apex_ws;
pub(crate) mod crg_ws;
mod proxy_server;

use clap::Parser;
use proxy_server::*;

/// A WebSocket server that proxies the WebSocket data from the CRG Scoreboard to additional clients.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[arg(long, default_value = "localhost")]
    crg_host: String,

    #[arg(long, default_value_t = 8000)]
    crg_port: u16,

    #[cfg(feature = "static-files")]
    #[arg(long)]
    files: Vec<String>,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    init_logging();

    #[cfg(not(feature = "static-files"))]
    let proxy = WsProxy::builder().crg(&args.crg_host, args.crg_port);
    #[cfg(feature = "static-files")]
    let mut proxy = WsProxy::builder().crg(&args.crg_host, args.crg_port);

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

    proxy.build().start().await
}
