use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::ipc::IpcRequest;
use crate::message::InboundMessage;

#[derive(Debug, Clone)]
pub struct BotInfo {
    pub id: i64,
    pub username: String,
}

pub type InboundHandler = broadcast::Sender<InboundMessage>;

#[async_trait]
pub trait TelegramClient: Send + Sync {
    async fn send_message(&self, chat_id: i64, request: &IpcRequest) -> Result<i64>;
    async fn get_me(&self) -> Result<BotInfo>;
    async fn start_polling(&self, handler: InboundHandler) -> Result<()>;
}
