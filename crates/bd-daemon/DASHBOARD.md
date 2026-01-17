# Beads Daemon Dashboard

Real-time monitoring dashboard for the jj-beads daemon. Provides HTTP endpoints for health checks, metrics, and statistics about daemon operations.

## Features

- **Health Check** - Simple health status endpoint
- **Prometheus Metrics** - Standardized metrics format for monitoring systems
- **Statistics API** - JSON stats for custom dashboards
- **Ready Tasks Query** - List of tasks ready for execution
- **Blocked Tasks Query** - Placeholder for future blocked tasks tracking
- **CORS Support** - Cross-origin requests enabled for web dashboards

## Enabling the Dashboard

Add the `dashboard` feature to your `Cargo.toml`:

```toml
[dependencies]
bd-daemon = { version = "0.1", features = ["dashboard"] }
```

## Usage Example

```rust
use bd_daemon::{Daemon, DaemonConfig};
use bd_daemon::dashboard::{DashboardServer, DaemonStats};
use bd_storage::Database;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize database
    let db = Database::open(".beads/turso.db").await?;
    let storage = Arc::new(db);

    // Create daemon with stats tracking
    let stats = Arc::new(DaemonStats::new());
    let daemon = Daemon::new(storage.clone(), ".beads")
        .with_stats(stats.clone());

    // Start dashboard server
    let addr = "127.0.0.1:8080".parse().unwrap();
    let dashboard = DashboardServer::new(addr, stats.clone(), Some(storage.clone()));
    dashboard.start().await?;

    println!("Dashboard running at http://127.0.0.1:8080");

    // Run daemon (blocks)
    daemon.run().await?;

    Ok(())
}
```

## HTTP Endpoints

### Root Page: `GET /`

HTML landing page with links to all endpoints.

**Example:**
```bash
curl http://localhost:8080/
```

### Health Check: `GET /health`

Returns server health and uptime.

**Response:**
```json
{
  "status": "ok",
  "uptime_seconds": 3600
}
```

**Example:**
```bash
curl http://localhost:8080/health
```

### Metrics: `GET /metrics`

Prometheus-compatible metrics endpoint.

**Metrics:**
- `beads_tasks_synced` - Total tasks synced since startup (counter)
- `beads_deps_synced` - Total dependencies synced since startup (counter)
- `beads_sync_errors` - Total sync errors encountered (counter)
- `beads_queue_depth` - Current change queue depth (gauge)
- `beads_last_sync_duration_microseconds` - Duration of last sync operation (gauge)
- `beads_uptime_seconds` - Daemon uptime in seconds (counter)

**Example Response:**
```
# HELP beads_tasks_synced Total tasks synced since startup
# TYPE beads_tasks_synced counter
beads_tasks_synced 42

# HELP beads_queue_depth Current change queue depth
# TYPE beads_queue_depth gauge
beads_queue_depth 3
```

**Example:**
```bash
curl http://localhost:8080/metrics
```

### Statistics: `GET /stats`

JSON statistics summary.

**Response:**
```json
{
  "tasks_synced": 42,
  "deps_synced": 15,
  "sync_errors": 0,
  "queue_depth": 3,
  "last_sync_duration_us": 1500,
  "uptime_seconds": 3600
}
```

**Example:**
```bash
curl http://localhost:8080/stats | jq
```

### Ready Tasks: `GET /ready`

Query tasks ready for execution (requires database connection).

**Response:**
```json
{
  "count": 5,
  "tasks": [
    "task-001",
    "task-002",
    "task-003",
    "task-004",
    "task-005"
  ]
}
```

**Example:**
```bash
curl http://localhost:8080/ready | jq
```

### Blocked Tasks: `GET /blocked`

Placeholder for blocked tasks query (not yet fully implemented).

**Response:**
```json
{
  "count": 0,
  "message": "Blocked tasks query not yet implemented. Use ready endpoint instead."
}
```

**Example:**
```bash
curl http://localhost:8080/blocked | jq
```

## Statistics Tracking

The `DaemonStats` struct tracks real-time metrics:

```rust
use bd_daemon::dashboard::DaemonStats;
use std::sync::Arc;
use std::time::Duration;

let stats = Arc::new(DaemonStats::new());

// Increment counters
stats.increment_tasks_synced();
stats.increment_deps_synced();
stats.increment_sync_errors();

// Update gauges
stats.set_queue_depth(5);
stats.record_sync_duration(Duration::from_millis(150));

// Query uptime
let uptime = stats.uptime();
```

## Prometheus Integration

The dashboard exposes metrics in Prometheus format. Add to your `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'beads-daemon'
    scrape_interval: 5s
    static_configs:
      - targets: ['localhost:8080']
```

## Architecture

- **Non-blocking** - Dashboard runs in a separate tokio task
- **Optional** - Daemon works without dashboard (feature-gated)
- **Thread-safe** - Uses atomic counters for lock-free stats updates
- **CORS-enabled** - Supports web-based monitoring dashboards

## Implementation Notes

### Statistics Storage

All counters use `AtomicU64` with `Relaxed` ordering for lock-free, high-performance updates:

```rust
pub struct DaemonStats {
    pub tasks_synced: Arc<AtomicU64>,
    pub deps_synced: Arc<AtomicU64>,
    pub sync_errors: Arc<AtomicU64>,
    pub queue_depth: Arc<AtomicU64>,
    pub last_sync_duration_us: Arc<AtomicU64>,
    pub uptime_started: Instant,
}
```

### HTTP Server

Built with `axum` for high-performance async HTTP handling. Includes:
- Tower middleware for CORS
- JSON and HTML responses
- Error handling with appropriate status codes

### Future Enhancements

- [ ] WebSocket endpoint for real-time event streaming
- [ ] Blocked tasks query implementation
- [ ] Detailed task state transitions
- [ ] Dependency graph visualization endpoint
- [ ] Historical metrics (time-series data)
- [ ] Authentication/authorization

## Testing

Run tests with the dashboard feature enabled:

```bash
cargo test -p bd-daemon --features dashboard
```

Run tests without dashboard:

```bash
cargo test -p bd-daemon
```

## Performance Impact

The dashboard adds minimal overhead:
- Atomic counter updates: ~1-5 nanoseconds per operation
- HTTP server runs in separate task (non-blocking)
- No locks in hot path
- Metrics collection is zero-cost when dashboard is disabled

## License

Same as the parent project (MIT OR Apache-2.0).
