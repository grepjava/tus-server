use std::convert::Infallible;

use axum::{
    extract::{Path, State},
    response::sse::{Event, KeepAlive, Sse},
};
use futures::Stream;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

use crate::app_state::AppState;

pub async fn stream_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.event_tx.subscribe();

    let stream = BroadcastStream::new(rx).filter_map(move |result| {
        let id = id.clone();
        match result {
            Ok(event) if event.upload_id == id => {
                let data = serde_json::to_string(&event).unwrap_or_default();
                Some(Ok(Event::default().data(data)))
            }
            _ => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
