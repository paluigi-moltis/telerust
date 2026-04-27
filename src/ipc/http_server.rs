use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use futures::stream::Stream;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tracing::{debug, error, info};

use crate::ipc::{IpcRequest, IpcResponse};
use crate::message::MessageBuffer;
use crate::telegram::TelegramClient;

pub struct HttpServerState {
    pub telegram: Arc<dyn TelegramClient>,
    pub chat_id: i64,
    pub message_buffer: Arc<MessageBuffer>,
}

pub fn router(state: Arc<HttpServerState>) -> Router {
    Router::new()
        .route("/send", post(handle_send))
        .route("/events", get(handle_events))
        .with_state(state)
}

async fn handle_send(
    State(state): State<Arc<HttpServerState>>,
    Json(request): Json<IpcRequest>,
) -> Json<IpcResponse> {
    debug!("Received IPC send request: {} chars", request.text.len());

    match state.telegram.send_message(state.chat_id, &request).await {
        Ok(message_id) => {
            info!("Message sent successfully: {message_id}");
            Json(IpcResponse::Ok { message_id })
        }
        Err(e) => {
            error!("Failed to send message: {e}");
            Json(IpcResponse::Error {
                error: e.to_string(),
            })
        }
    }
}

async fn handle_events(
    State(state): State<Arc<HttpServerState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    debug!("New SSE subscriber connected");
    let rx = state.message_buffer.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(msg) => {
            let json = serde_json::to_string(&msg).unwrap_or_default();
            Some(Ok(Event::default().data(json)))
        }
        Err(_) => None,
    });
    Sse::new(stream)
}

/// Start the HTTP server on 127.0.0.1 at the given port.
/// Returns the actual port the server bound to (useful when binding to port 0 for tests).
pub async fn start(state: Arc<HttpServerState>, port: u16) -> anyhow::Result<u16> {
    let app = router(state);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let actual_port = listener.local_addr()?.port();
    info!("HTTP IPC server listening on 127.0.0.1:{actual_port}");
    axum::serve(listener, app).await?;
    Ok(actual_port)
}
