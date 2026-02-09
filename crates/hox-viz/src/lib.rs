//! # hox-viz
//!
//! 3D cyberpunk orchestration visualization for Hox.
//! Serves a Three.js-based force-directed graph via an embedded Axum web server.

mod assets;
mod server;
mod sse;
mod state;

pub use state::{VizDelta, VizLink, VizNode, VizState, LinkType, NodeType};

use tracing::info;

/// Configuration for the visualization server
#[derive(Debug, Clone)]
pub struct VizConfig {
    /// Port to serve on
    pub port: u16,
    /// Refresh interval in milliseconds for SSE updates
    pub refresh_ms: u64,
    /// Maximum oplog entries to track
    pub max_oplog: usize,
    /// Open browser automatically on launch
    pub open_browser: bool,
}

impl Default for VizConfig {
    fn default() -> Self {
        Self {
            port: 7070,
            refresh_ms: 500,
            max_oplog: 100,
            open_browser: true,
        }
    }
}

/// Run the visualization server
pub async fn run(config: VizConfig) -> anyhow::Result<()> {
    let addr = format!("0.0.0.0:{}", config.port);
    let url = format!("http://localhost:{}", config.port);

    info!("Starting hox-viz server on {}", addr);

    let open_browser = config.open_browser;

    // Spawn browser opener
    if open_browser {
        let url_clone = url.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if let Err(e) = open::that(&url_clone) {
                eprintln!("Failed to open browser: {}", e);
            }
        });
    }

    println!("Hox Viz running at {}", url);
    println!("Press Ctrl+C to stop");

    server::serve(config, &addr).await
}
