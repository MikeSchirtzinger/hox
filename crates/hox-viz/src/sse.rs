//! Server-Sent Events endpoint for real-time updates

use crate::{
    server::SharedState,
    state::{self, VizState},
};
use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
};
use futures::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;

/// Maximum number of uncached changes to fetch file diffs for per poll cycle.
const MAX_FILE_DIFF_FETCHES_PER_CYCLE: usize = 5;

/// SSE handler - streams state updates to the frontend
pub async fn sse_handler(
    State(app): State<SharedState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let refresh_ms = app.config.refresh_ms;

    let stream = async_stream::stream! {
        let mut last_state: Option<VizState> = None;
        let mut tick_count: u64 = 0;
        let resync_interval = 5000 / refresh_ms.max(1); // Full resync every ~5s

        loop {
            let dashboard_result = app.data_source.fetch_state().await;

            match dashboard_result {
                Ok(mut dashboard_state) => {
                    // Populate file diffs for DAG changes using the cache
                    if let Some(ref mut dag) = dashboard_state.dag {
                        // Collect change_ids not yet in cache
                        let uncached: Vec<String> = {
                            let cache = app.file_cache.read().await;
                            dag.changes
                                .iter()
                                .filter(|c| !cache.contains_key(&c.change_id))
                                .map(|c| c.change_id.clone())
                                .take(MAX_FILE_DIFF_FETCHES_PER_CYCLE)
                                .collect()
                        };

                        // Fetch diffs concurrently for uncached changes
                        if !uncached.is_empty() {
                            let fetch_futures: Vec<_> = uncached
                                .iter()
                                .map(|id| hox_dashboard::fetch_file_diff(id))
                                .collect();

                            let results = futures::future::join_all(fetch_futures).await;

                            let mut cache = app.file_cache.write().await;
                            for (change_id, result) in uncached.iter().zip(results) {
                                let files = result.unwrap_or_default();
                                cache.insert(change_id.clone(), files);
                            }
                        }

                        // Merge cached files into dag.changes
                        {
                            let cache = app.file_cache.read().await;
                            for change in dag.changes.iter_mut() {
                                if let Some(files) = cache.get(&change.change_id) {
                                    change.files = files.clone();
                                }
                            }
                        }
                    }

                    let viz_state = state::translate(&dashboard_state);

                    // Full snapshot on first connect or every resync_interval
                    if last_state.is_none() || tick_count % resync_interval == 0 {
                        if let Ok(json) = serde_json::to_string(&viz_state) {
                            yield Ok(Event::default().event("state").data(json));
                        }
                    } else if let Some(ref old) = last_state {
                        // Send delta
                        let delta = state::compute_delta(old, &viz_state);
                        if !delta.changed_nodes.is_empty()
                            || !delta.new_oplog.is_empty()
                            || !delta.changed_phases.is_empty()
                        {
                            if let Ok(json) = serde_json::to_string(&delta) {
                                yield Ok(Event::default().event("update").data(json));
                            }
                        }

                        // Send individual oplog entries for immediate effects
                        for entry in &delta.new_oplog {
                            if let Ok(json) = serde_json::to_string(entry) {
                                yield Ok(Event::default().event("oplog").data(json));
                            }
                        }
                    }

                    *app.current_state.write().await = Some(viz_state.clone());
                    last_state = Some(viz_state);
                }
                Err(e) => {
                    eprintln!("[hox-viz] fetch_state error: {e}");
                    // On error, preserve last known state rather than sending empty
                    if let Some(ref cached) = last_state {
                        if let Ok(json) = serde_json::to_string(cached) {
                            yield Ok(Event::default().event("state").data(json));
                        }
                    }
                    // If no previous state, send empty so frontend at least connects
                    else {
                        let empty = VizState {
                            session: Default::default(),
                            metrics: Default::default(),
                            nodes: vec![],
                            links: vec![],
                            phases: vec![],
                            oplog: vec![],
                            view_mode: "orchestration".to_string(),
                        };
                        if let Ok(json) = serde_json::to_string(&empty) {
                            yield Ok(Event::default().event("state").data(json));
                        }
                    }
                }
            }

            tick_count += 1;
            tokio::time::sleep(Duration::from_millis(refresh_ms)).await;
        }
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}
