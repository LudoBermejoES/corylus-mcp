//! `corylus-mcp` — launched by an MCP client as a stdio MCP server.
//!
//! Usage: `corylus-mcp [--app-data <dir>]`. Without `--app-data` it uses
//! Corylus's own data directory for this OS.

use std::path::PathBuf;

use corylus_mcp_bridge::Bridge;
use rmcp::ServiceExt;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let mut app_data: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--app-data" => app_data = args.next().map(PathBuf::from),
            "--version" => {
                println!("corylus-mcp {} (protocol {})", env!("CARGO_PKG_VERSION"), corylus_mcp::PROTOCOL_VERSION);
                return;
            }
            other => {
                eprintln!("corylus-mcp: unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let Some(app_data) = app_data.or_else(corylus_mcp::endpoint::default_app_data_dir) else {
        eprintln!("corylus-mcp: cannot find Corylus's data directory");
        std::process::exit(1);
    };

    let bridge = Bridge::new(app_data);
    let running = match bridge.clone().serve(rmcp::transport::stdio()).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("corylus-mcp: {e}");
            std::process::exit(1);
        }
    };
    let _ = running.waiting().await;
    bridge.goodbye().await;
}
