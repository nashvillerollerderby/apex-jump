use clap::Parser;
use serde::{Deserialize, Serialize};

/// Runs a WebSocket server that proxies the WebSocket data from the CRG Scoreboard to additional clients.
#[derive(Parser, Debug, Serialize, Deserialize)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// The host server for the CRG Scoreboard instance
    #[arg(long, default_value = "localhost")]
    pub crg_host: String,

    /// The host port for the CRG Scoreboard instance
    #[arg(long, default_value_t = 8000)]
    pub crg_port: u16,

    /// The number of seconds to wait between attempts to reconnect to the CRG Scoreboard WebSocket
    #[arg(short = 'r', long = "reconnect-delay-s", default_value_t = 5)]
    pub crg_ws_reconnect_s: u64,

    #[serde(skip)]
    #[arg(short = 'c', long = "config")]
    pub config_file: Option<String>,

    /// The port on which the apex-jump server should be started
    #[arg(short, long, default_value_t = 8001)]
    pub port: u16,

    /// File mount formatted as "<path_to_files>"
    #[cfg(feature = "static-files")]
    #[arg(short, long)]
    pub files: Option<String>,

    #[arg(long)]
    pub registration_paths: Vec<String>,
}
