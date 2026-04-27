use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tracing::{debug, error, info};

use crate::ipc::{IpcRequest, IpcResponse};
use crate::message::MessageBuffer;
use crate::telegram::TelegramClient;

/// Start the send socket server.
/// Each connection: read one JSON line, process, write JSON response, close.
pub async fn start_send_server(
    path: &Path,
    telegram: Arc<dyn TelegramClient>,
    chat_id: i64,
) -> Result<()> {
    let _ = std::fs::remove_file(path);

    let listener = UnixListener::bind(path)
        .with_context(|| format!("Failed to bind Unix socket: {}", path.display()))?;

    info!("Unix send socket listening on {}", path.display());

    loop {
        let (stream, _) = listener.accept().await?;
        let telegram = telegram.clone();
        tokio::spawn(async move {
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();

            match reader.read_line(&mut line).await {
                Ok(0) => return,
                Ok(_) => {}
                Err(e) => {
                    error!("Failed to read from Unix socket: {e}");
                    return;
                }
            }

            let response = match serde_json::from_str::<IpcRequest>(&line) {
                Ok(request) => {
                    debug!("Unix socket send request: {} chars", request.text.len());
                    match telegram.send_message(chat_id, &request).await {
                        Ok(message_id) => IpcResponse::Ok { message_id },
                        Err(e) => IpcResponse::Error {
                            error: e.to_string(),
                        },
                    }
                }
                Err(e) => IpcResponse::Error {
                    error: format!("Invalid JSON: {e}"),
                },
            };

            let json = serde_json::to_string(&response).unwrap_or_default();
            let _ = writer.write_all(json.as_bytes()).await;
            let _ = writer.write_all(b"\n").await;
            let _ = writer.shutdown().await;
        });
    }
}

/// Start the events socket server.
/// Each connection: stream NDJSON of inbound messages until disconnect.
pub async fn start_events_server(path: &Path, message_buffer: Arc<MessageBuffer>) -> Result<()> {
    let _ = std::fs::remove_file(path);

    let listener = UnixListener::bind(path)
        .with_context(|| format!("Failed to bind Unix events socket: {}", path.display()))?;

    info!("Unix events socket listening on {}", path.display());

    loop {
        let (stream, _) = listener.accept().await?;
        let mut rx = message_buffer.subscribe();
        tokio::spawn(async move {
            let (_, mut writer) = stream.into_split();
            debug!("New Unix events subscriber connected");

            loop {
                match rx.recv().await {
                    Ok(msg) => {
                        let mut json = serde_json::to_string(&msg).unwrap_or_default();
                        json.push('\n');
                        if writer.write_all(json.as_bytes()).await.is_err() {
                            debug!("Unix events subscriber disconnected");
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        debug!("Unix events subscriber lagged by {n} messages");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        });
    }
}

/// Clean up socket files (call on shutdown).
pub fn cleanup_sockets(send_path: &Path, events_path: &Path) {
    for path in [send_path, events_path] {
        if let Err(e) = std::fs::remove_file(path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("Failed to remove socket {}: {e}", path.display());
            }
        }
    }
}
