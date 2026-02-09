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
                Ok(dashboard_state) => {
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
                Err(_) => {
                    // Send empty state on error
                    let empty = VizState {
                        session: Default::default(),
                        metrics: Default::default(),
                        nodes: vec![],
                        links: vec![],
                        phases: vec![],
                        oplog: vec![],
                    };
                    if let Ok(json) = serde_json::to_string(&empty) {
                        yield Ok(Event::default().event("state").data(json));
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
