//! Axum web server for the visualization

use crate::{sse, state, VizConfig};
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

/// Shared application state
pub struct AppState {
    pub config: VizConfig,
    pub current_state: RwLock<Option<state::VizState>>,
    pub data_source: hox_dashboard::JjDataSource,
}

pub type SharedState = Arc<AppState>;

/// Serve the visualization
pub async fn serve(config: VizConfig, addr: &str) -> anyhow::Result<()> {
    let dashboard_config = hox_dashboard::DashboardConfig {
        refresh_ms: config.refresh_ms,
        max_oplog_entries: config.max_oplog,
        local_time: true,
        metrics_path: None,
    };

    let app_state = Arc::new(AppState {
        config,
        current_state: RwLock::new(None),
        data_source: hox_dashboard::JjDataSource::new(dashboard_config),
    });

    let app = Router::new()
        .route("/api/state", get(get_state))
        .route("/api/events", get(sse::sse_handler))
        .route("/api/health", get(health))
        .fallback(crate::assets::static_handler)
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// GET /api/state - Returns full current state
async fn get_state(State(app): State<SharedState>) -> Result<Json<state::VizState>, StatusCode> {
    match app.data_source.fetch_state().await {
        Ok(dashboard_state) => {
            let viz_state = state::translate(&dashboard_state);
            *app.current_state.write().await = Some(viz_state.clone());
            Ok(Json(viz_state))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// GET /api/health
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "hox-viz"
    }))
}
