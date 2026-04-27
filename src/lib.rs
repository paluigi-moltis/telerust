pub mod config;
pub mod daemon;
pub mod ipc;
pub mod message;
pub mod pairing;
pub mod secret;
pub mod telegram;

pub use config::Config;
pub use message::{InboundMessage, MessageBuffer};
pub use secret::{resolve_token, TokenSource};
pub use telegram::{BotInfo, TelegramClient};

use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::Notify;
use tracing::info;

use crate::ipc::http_server::HttpServerState;
use crate::ipc::unix_server;

/// Shared runtime state for the bot.
pub struct BotState {
    pub config: Config,
    pub token: String,
    pub token_source: TokenSource,
    pub telegram: Arc<dyn TelegramClient>,
    pub message_buffer: Arc<MessageBuffer>,
}

/// Handle to request graceful shutdown.
pub struct ShutdownHandle {
    notify: Arc<Notify>,
}

impl ShutdownHandle {
    pub fn shutdown(&self) {
        self.notify.notify_waiters();
    }
}

/// The main bot struct. Owns all state and spawns all tasks.
pub struct TelerustBot {
    state: Arc<BotState>,
    shutdown: Arc<Notify>,
}

impl TelerustBot {
    pub fn new(
        config: Config,
        token: String,
        token_source: TokenSource,
        telegram: Arc<dyn TelegramClient>,
    ) -> Self {
        let message_buffer = Arc::new(MessageBuffer::new(256));
        let state = Arc::new(BotState {
            config,
            token,
            token_source,
            telegram,
            message_buffer,
        });
        let shutdown = Arc::new(Notify::new());
        Self { state, shutdown }
    }

    pub fn shutdown_handle(&self) -> ShutdownHandle {
        ShutdownHandle {
            notify: self.shutdown.clone(),
        }
    }

    pub async fn run(&self) -> Result<()> {
        let paired_user_id = self
            .state
            .config
            .paired
            .user_id
            .context("No paired user_id configured. Run `telerust pair` first.")?;

        // Spawn HTTP server
        let http_state = Arc::new(HttpServerState {
            telegram: self.state.telegram.clone(),
            chat_id: paired_user_id,
            message_buffer: self.state.message_buffer.clone(),
        });
        let http_port = self.state.config.ipc.http_port;
        tokio::spawn(async move {
            if let Err(e) = ipc::http_server::start(http_state, http_port).await {
                tracing::error!("HTTP server error: {e}");
            }
        });

        // Spawn Unix send socket
        let send_path = self.state.config.ipc.unix_socket_path.clone();
        let telegram = self.state.telegram.clone();
        let chat_id = paired_user_id;
        tokio::spawn(async move {
            if let Err(e) = ipc::unix_server::start_send_server(&send_path, telegram, chat_id).await
            {
                tracing::error!("Unix send server error: {e}");
            }
        });

        // Spawn Unix events socket
        let events_path = self.state.config.events_socket_path();
        let buffer = self.state.message_buffer.clone();
        tokio::spawn(async move {
            if let Err(e) = ipc::unix_server::start_events_server(&events_path, buffer).await {
                tracing::error!("Unix events server error: {e}");
            }
        });

        // Spawn Telegram polling
        let handler = self.state.message_buffer.sender();
        let telegram = self.state.telegram.clone();
        tokio::spawn(async move {
            if let Err(e) = telegram.start_polling(handler).await {
                tracing::error!("Telegram polling error: {e}");
            }
        });

        info!("Telerust bot running. Waiting for shutdown signal...");
        self.shutdown.notified().await;

        // Clean up socket files
        let send_path = self.state.config.ipc.unix_socket_path.clone();
        let events_path = self.state.config.events_socket_path();
        unix_server::cleanup_sockets(&send_path, &events_path);

        info!("Shutdown signal received. Cleaning up...");
        Ok(())
    }
}
