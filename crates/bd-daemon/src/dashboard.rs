//! Real-time monitoring dashboard for the daemon.
//!
//! Provides HTTP endpoints for health checks, metrics, and statistics about daemon operations.
//! Useful for monitoring sync operations, queue depth, and system health.

use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

/// Statistics tracked by the daemon.
#[derive(Debug, Clone)]
pub struct DaemonStats {
    /// Total number of tasks synced since startup
    pub tasks_synced: Arc<AtomicU64>,
    /// Total number of dependencies synced since startup
    pub deps_synced: Arc<AtomicU64>,
    /// Total number of sync errors encountered
    pub sync_errors: Arc<AtomicU64>,
    /// Current depth of the change queue
    pub queue_depth: Arc<AtomicU64>,
    /// Duration of the last sync operation in microseconds
    pub last_sync_duration_us: Arc<AtomicU64>,
    /// When the daemon started
    pub uptime_started: Instant,
}

impl DaemonStats {
    /// Create new daemon statistics tracker.
    pub fn new() -> Self {
        Self {
            tasks_synced: Arc::new(AtomicU64::new(0)),
            deps_synced: Arc::new(AtomicU64::new(0)),
            sync_errors: Arc::new(AtomicU64::new(0)),
            queue_depth: Arc::new(AtomicU64::new(0)),
            last_sync_duration_us: Arc::new(AtomicU64::new(0)),
            uptime_started: Instant::now(),
        }
    }

    /// Increment tasks synced counter.
    pub fn increment_tasks_synced(&self) {
        self.tasks_synced.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment dependencies synced counter.
    pub fn increment_deps_synced(&self) {
        self.deps_synced.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment sync errors counter.
    pub fn increment_sync_errors(&self) {
        self.sync_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Update queue depth.
    pub fn set_queue_depth(&self, depth: u64) {
        self.queue_depth.store(depth, Ordering::Relaxed);
    }

    /// Record the duration of a sync operation.
    pub fn record_sync_duration(&self, duration: Duration) {
        self.last_sync_duration_us
            .store(duration.as_micros() as u64, Ordering::Relaxed);
    }

    /// Get uptime duration.
    pub fn uptime(&self) -> Duration {
        self.uptime_started.elapsed()
    }
}

impl Default for DaemonStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared state for dashboard server.
#[derive(Clone)]
struct DashboardState {
    stats: Arc<DaemonStats>,
    storage: Option<Arc<RwLock<bd_storage::Database>>>,
}

/// Dashboard HTTP server for real-time monitoring.
pub struct DashboardServer {
    bind_addr: SocketAddr,
    state: DashboardState,
}

impl DashboardServer {
    /// Create a new dashboard server.
    ///
    /// # Arguments
    /// * `bind_addr` - Address to bind the HTTP server to
    /// * `stats` - Shared daemon statistics
    /// * `storage` - Optional database reference for querying task states
    pub fn new(
        bind_addr: SocketAddr,
        stats: Arc<DaemonStats>,
        storage: Option<Arc<RwLock<bd_storage::Database>>>,
    ) -> Self {
        Self {
            bind_addr,
            state: DashboardState { stats, storage },
        }
    }

    /// Start the dashboard server.
    ///
    /// Returns immediately after starting the server in a background task.
    /// The server will run until the tokio runtime is shutdown.
    pub async fn start(self) -> Result<(), Box<dyn std::error::Error>> {
        let app = Router::new()
            .route("/", get(handle_root))
            .route("/health", get(handle_health))
            .route("/metrics", get(handle_metrics))
            .route("/stats", get(handle_stats))
            .route("/ready", get(handle_ready))
            .route("/blocked", get(handle_blocked))
            .layer(CorsLayer::permissive())
            .with_state(self.state.clone());

        info!("Starting dashboard server on http://{}", self.bind_addr);

        tokio::spawn(async move {
            let listener = match tokio::net::TcpListener::bind(self.bind_addr).await {
                Ok(l) => l,
                Err(e) => {
                    error!("Failed to bind dashboard server: {}", e);
                    return;
                }
            };

            if let Err(e) = axum::serve(listener, app).await {
                error!("Dashboard server error: {}", e);
            }
        });

        Ok(())
    }
}

/// Root handler - shows basic dashboard information.
async fn handle_root() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Beads Daemon Dashboard</title>
    <style>
        body { font-family: sans-serif; margin: 40px; background: #f5f5f5; }
        .container { max-width: 800px; margin: 0 auto; background: white; padding: 30px; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }
        h1 { color: #333; border-bottom: 2px solid #007bff; padding-bottom: 10px; }
        .endpoints { list-style: none; padding: 0; }
        .endpoints li { margin: 15px 0; padding: 10px; background: #f8f9fa; border-left: 4px solid #007bff; }
        code { background: #e9ecef; padding: 2px 6px; border-radius: 3px; font-family: 'Courier New', monospace; }
        a { color: #007bff; text-decoration: none; }
        a:hover { text-decoration: underline; }
    </style>
</head>
<body>
    <div class="container">
        <h1>🔧 Beads Daemon Dashboard</h1>
        <p>Real-time monitoring endpoints for the beads daemon.</p>

        <h2>Available Endpoints</h2>
        <ul class="endpoints">
            <li><a href="/health"><code>GET /health</code></a> - Health check (JSON)</li>
            <li><a href="/metrics"><code>GET /metrics</code></a> - Prometheus metrics (text)</li>
            <li><a href="/stats"><code>GET /stats</code></a> - Statistics summary (JSON)</li>
            <li><a href="/ready"><code>GET /ready</code></a> - Ready tasks count (JSON)</li>
            <li><a href="/blocked"><code>GET /blocked</code></a> - Blocked tasks (JSON)</li>
        </ul>
    </div>
</body>
</html>"#,
    )
}

/// Health check endpoint.
async fn handle_health(State(state): State<DashboardState>) -> Json<HealthResponse> {
    let uptime_secs = state.stats.uptime().as_secs();

    Json(HealthResponse {
        status: "ok".to_string(),
        uptime_seconds: uptime_secs,
    })
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    uptime_seconds: u64,
}

/// Prometheus-style metrics endpoint.
async fn handle_metrics(State(state): State<DashboardState>) -> Response {
    let tasks_synced = state.stats.tasks_synced.load(Ordering::Relaxed);
    let deps_synced = state.stats.deps_synced.load(Ordering::Relaxed);
    let sync_errors = state.stats.sync_errors.load(Ordering::Relaxed);
    let queue_depth = state.stats.queue_depth.load(Ordering::Relaxed);
    let last_sync_us = state.stats.last_sync_duration_us.load(Ordering::Relaxed);
    let uptime_secs = state.stats.uptime().as_secs();

    let metrics = format!(
        r#"# HELP beads_tasks_synced Total tasks synced since startup
# TYPE beads_tasks_synced counter
beads_tasks_synced {}

# HELP beads_deps_synced Total dependencies synced since startup
# TYPE beads_deps_synced counter
beads_deps_synced {}

# HELP beads_sync_errors Total sync errors encountered
# TYPE beads_sync_errors counter
beads_sync_errors {}

# HELP beads_queue_depth Current change queue depth
# TYPE beads_queue_depth gauge
beads_queue_depth {}

# HELP beads_last_sync_duration_microseconds Duration of last sync operation
# TYPE beads_last_sync_duration_microseconds gauge
beads_last_sync_duration_microseconds {}

# HELP beads_uptime_seconds Daemon uptime in seconds
# TYPE beads_uptime_seconds counter
beads_uptime_seconds {}
"#,
        tasks_synced, deps_synced, sync_errors, queue_depth, last_sync_us, uptime_secs
    );

    (StatusCode::OK, [("Content-Type", "text/plain")], metrics).into_response()
}

/// Statistics summary endpoint (JSON).
async fn handle_stats(State(state): State<DashboardState>) -> Json<StatsResponse> {
    let tasks_synced = state.stats.tasks_synced.load(Ordering::Relaxed);
    let deps_synced = state.stats.deps_synced.load(Ordering::Relaxed);
    let sync_errors = state.stats.sync_errors.load(Ordering::Relaxed);
    let queue_depth = state.stats.queue_depth.load(Ordering::Relaxed);
    let last_sync_us = state.stats.last_sync_duration_us.load(Ordering::Relaxed);
    let uptime = state.stats.uptime();

    Json(StatsResponse {
        tasks_synced,
        deps_synced,
        sync_errors,
        queue_depth,
        last_sync_duration_us: last_sync_us,
        uptime_seconds: uptime.as_secs(),
    })
}

#[derive(Serialize)]
struct StatsResponse {
    tasks_synced: u64,
    deps_synced: u64,
    sync_errors: u64,
    queue_depth: u64,
    last_sync_duration_us: u64,
    uptime_seconds: u64,
}

/// Ready tasks count endpoint (requires database).
async fn handle_ready(State(state): State<DashboardState>) -> Response {
    let Some(storage) = &state.storage else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Database not available".to_string(),
            }),
        )
            .into_response();
    };

    // Use default options for ready tasks
    let opts = bd_storage::ReadyTasksOptions::default();
    let db = storage.read().await;
    match db.get_ready_tasks(opts).await {
        Ok(tasks) => Json(ReadyResponse {
            count: tasks.len(),
            tasks: tasks.into_iter().map(|t| t.id).collect(),
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to query ready tasks: {}", e),
            }),
        )
            .into_response(),
    }
}

#[derive(Serialize)]
struct ReadyResponse {
    count: usize,
    tasks: Vec<String>,
}

/// Blocked tasks count endpoint (requires database).
/// Returns count of tasks marked as blocked in the database.
async fn handle_blocked(State(state): State<DashboardState>) -> Response {
    let Some(_storage) = &state.storage else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Database not available".to_string(),
            }),
        )
            .into_response();
    };

    // For now, return a simple count from blocked_cache table
    // In the future, we could query the blocked_cache table directly
    // For simplicity, we'll return a placeholder response
    Json(BlockedResponse {
        count: 0,
        message: "Blocked tasks query not yet implemented. Use ready endpoint instead."
            .to_string(),
    })
    .into_response()
}

#[derive(Serialize)]
struct BlockedResponse {
    count: usize,
    message: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use bd_storage::Database;
    use std::time::Duration;
    use tempfile::TempDir;

    async fn create_test_db() -> (Database, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Database::open(&db_path).await.unwrap();
        db.init_schema().await.unwrap();
        (db, temp_dir)
    }

    #[tokio::test]
    async fn test_daemon_stats_creation() {
        let stats = DaemonStats::new();
        assert_eq!(stats.tasks_synced.load(Ordering::Relaxed), 0);
        assert_eq!(stats.deps_synced.load(Ordering::Relaxed), 0);
        assert_eq!(stats.sync_errors.load(Ordering::Relaxed), 0);
        assert_eq!(stats.queue_depth.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_daemon_stats_increment() {
        let stats = DaemonStats::new();

        stats.increment_tasks_synced();
        stats.increment_tasks_synced();
        assert_eq!(stats.tasks_synced.load(Ordering::Relaxed), 2);

        stats.increment_deps_synced();
        assert_eq!(stats.deps_synced.load(Ordering::Relaxed), 1);

        stats.increment_sync_errors();
        assert_eq!(stats.sync_errors.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_daemon_stats_queue_depth() {
        let stats = DaemonStats::new();

        stats.set_queue_depth(5);
        assert_eq!(stats.queue_depth.load(Ordering::Relaxed), 5);

        stats.set_queue_depth(10);
        assert_eq!(stats.queue_depth.load(Ordering::Relaxed), 10);
    }

    #[tokio::test]
    async fn test_daemon_stats_sync_duration() {
        let stats = DaemonStats::new();

        let duration = Duration::from_millis(150);
        stats.record_sync_duration(duration);

        let recorded_us = stats.last_sync_duration_us.load(Ordering::Relaxed);
        assert_eq!(recorded_us, 150_000); // 150ms = 150,000 microseconds
    }

    #[tokio::test]
    async fn test_daemon_stats_uptime() {
        let stats = DaemonStats::new();

        tokio::time::sleep(Duration::from_millis(100)).await;

        let uptime = stats.uptime();
        assert!(uptime >= Duration::from_millis(100));
    }

    #[tokio::test]
    async fn test_dashboard_server_creation() {
        let stats = Arc::new(DaemonStats::new());
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();

        let server = DashboardServer::new(addr, stats.clone(), None);
        assert_eq!(server.state.stats.tasks_synced.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_dashboard_health_endpoint() {
        let state = DashboardState {
            stats: Arc::new(DaemonStats::new()),
            storage: None,
        };

        let response = handle_health(State(state)).await;
        assert_eq!(response.0.status, "ok");
    }

    #[tokio::test]
    async fn test_dashboard_stats_endpoint() {
        let stats = Arc::new(DaemonStats::new());
        stats.increment_tasks_synced();
        stats.increment_tasks_synced();
        stats.increment_deps_synced();
        stats.set_queue_depth(3);

        let state = DashboardState {
            stats,
            storage: None,
        };

        let response = handle_stats(State(state)).await;
        assert_eq!(response.0.tasks_synced, 2);
        assert_eq!(response.0.deps_synced, 1);
        assert_eq!(response.0.queue_depth, 3);
    }

    #[tokio::test]
    async fn test_dashboard_ready_endpoint_without_storage() {
        let state = DashboardState {
            stats: Arc::new(DaemonStats::new()),
            storage: None,
        };

        let response = handle_ready(State(state)).await;
        // Should return 503 Service Unavailable when no storage
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_dashboard_blocked_endpoint_without_storage() {
        let state = DashboardState {
            stats: Arc::new(DaemonStats::new()),
            storage: None,
        };

        let response = handle_blocked(State(state)).await;
        // Should return 503 Service Unavailable when no storage
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_dashboard_metrics_format() {
        let stats = Arc::new(DaemonStats::new());
        stats.increment_tasks_synced();
        stats.set_queue_depth(5);

        let state = DashboardState {
            stats,
            storage: None,
        };

        let response = handle_metrics(State(state)).await;
        assert_eq!(response.status(), StatusCode::OK);

        // Extract body as string
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();

        // Verify Prometheus format
        assert!(body_str.contains("# HELP beads_tasks_synced"));
        assert!(body_str.contains("# TYPE beads_tasks_synced counter"));
        assert!(body_str.contains("beads_tasks_synced 1"));
        assert!(body_str.contains("beads_queue_depth 5"));
    }
}
