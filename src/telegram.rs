use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use teloxide::prelude::*;
use teloxide::types::{ParseMode as TeloxideParseMode, ReplyParameters};
use tokio::sync::broadcast;
use tracing::{error, info};

use crate::config::PairedUser;
use crate::ipc::IpcRequest;
use crate::ipc::ParseMode;
use crate::message::InboundMessage;
use crate::pairing::is_paired_user;

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

pub struct TeloxideClient {
    bot: teloxide::Bot,
    paired: PairedUser,
}

impl TeloxideClient {
    pub fn new(token: &str, paired: PairedUser) -> Self {
        let bot = teloxide::Bot::new(token);
        Self { bot, paired }
    }

    async fn send_plain(&self, chat_id: i64, request: &IpcRequest) -> Result<i64> {
        let mut msg = self
            .bot
            .send_message(teloxide::types::ChatId(chat_id), &request.text);
        if let Some(reply_id) = request.reply_to_message_id {
            let reply_params = ReplyParameters::new(teloxide::types::MessageId(reply_id as i32));
            msg = msg.reply_parameters(reply_params);
        }
        let result = msg.await.context("Failed to send Telegram message")?;
        Ok(result.id.0 as i64)
    }
}

#[async_trait]
impl TelegramClient for TeloxideClient {
    async fn send_message(&self, chat_id: i64, request: &IpcRequest) -> Result<i64> {
        let mut msg = self
            .bot
            .send_message(teloxide::types::ChatId(chat_id), &request.text);

        if let Some(ref mode) = request.parse_mode {
            msg = msg.parse_mode(match mode {
                ParseMode::MarkdownV2 => TeloxideParseMode::MarkdownV2,
                ParseMode::HTML => TeloxideParseMode::Html,
                ParseMode::Plain => {
                    return self.send_plain(chat_id, request).await;
                }
            });
        }

        if let Some(reply_id) = request.reply_to_message_id {
            let reply_params = ReplyParameters::new(teloxide::types::MessageId(reply_id as i32));
            msg = msg.reply_parameters(reply_params);
        }

        let result = msg.await.context("Failed to send Telegram message")?;
        Ok(result.id.0 as i64)
    }

    async fn get_me(&self) -> Result<BotInfo> {
        let me = self.bot.get_me().await.context("Failed to call getMe")?;
        Ok(BotInfo {
            id: me.id.0 as i64,
            username: me.username().to_string(),
        })
    }

    async fn start_polling(&self, handler: InboundHandler) -> Result<()> {
        let paired = self.paired.clone();
        let bot = self.bot.clone();

        let handler_fn = move |msg: Message| {
            let handler = handler.clone();
            let paired = paired.clone();
            async move {
                let sender_id = msg.from.as_ref().map(|u| u.id.0 as i64);
                let sender_username = msg.from.as_ref().and_then(|u| u.username.as_deref());

                if !is_paired_user(&paired, sender_id, sender_username) {
                    tracing::trace!("Ignoring message from non-paired user");
                    return respond(());
                }

                let text = msg.text().unwrap_or("").to_string();
                let inbound = InboundMessage {
                    message_id: msg.id.0 as i64,
                    text,
                    date: msg.date.timestamp(),
                    from_username: sender_username.map(String::from),
                };

                let _ = handler.send(inbound);
                respond(())
            }
        };

        info!("Starting Telegram long-polling...");
        let listener = teloxide::update_listeners::polling_default(bot.clone()).await;
        let error_handler = Arc::new(|err: teloxide::RequestError| {
            error!("Telegram polling error: {err:?}");
            std::future::ready(())
        });

        Dispatcher::builder(bot, Update::filter_message().endpoint(handler_fn))
            .build()
            .dispatch_with_listener(listener, error_handler)
            .await;

        Ok(())
    }
}
