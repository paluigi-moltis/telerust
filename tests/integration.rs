use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;

use telerust::config::{Config, IpcConfig, PairedUser};
use telerust::ipc::IpcRequest;
use telerust::message::InboundMessage;
use telerust::telegram::{BotInfo, InboundHandler, TelegramClient};
use telerust::{TelerustBot, TokenSource};

struct MockTelegramClient {
    sent_messages: Arc<Mutex<Vec<(i64, IpcRequest)>>>,
    inbound_tx: Arc<Mutex<Option<InboundHandler>>>,
}

impl MockTelegramClient {
    fn new() -> Self {
        Self {
            sent_messages: Arc::new(Mutex::new(Vec::new())),
            inbound_tx: Arc::new(Mutex::new(None)),
        }
    }

    fn sent(&self) -> Vec<(i64, IpcRequest)> {
        self.sent_messages.lock().unwrap().clone()
    }

    fn simulate_inbound(&self, msg: InboundMessage) {
        if let Some(ref tx) = *self.inbound_tx.lock().unwrap() {
            let _ = tx.send(msg);
        }
    }
}

#[async_trait]
impl TelegramClient for MockTelegramClient {
    async fn send_message(&self, chat_id: i64, request: &IpcRequest) -> Result<i64> {
        self.sent_messages
            .lock()
            .unwrap()
            .push((chat_id, request.clone()));
        Ok(12345)
    }

    async fn get_me(&self) -> Result<BotInfo> {
        Ok(BotInfo {
            id: 999,
            username: "test_bot".to_string(),
        })
    }

    async fn start_polling(&self, handler: InboundHandler) -> Result<()> {
        *self.inbound_tx.lock().unwrap() = Some(handler);
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    }
}

fn test_config(port: u16, socket_dir: &std::path::Path) -> Config {
    Config {
        bot_token: Some("test-token".to_string()),
        use_keyring: false,
        paired: PairedUser {
            username: Some("testuser".to_string()),
            user_id: Some(42),
        },
        ipc: IpcConfig {
            unix_socket_path: socket_dir.join(format!("telerust-test-{port}.sock")),
            http_port: port,
        },
    }
}

#[tokio::test]
async fn test_http_send_reaches_telegram() {
    let mock = Arc::new(MockTelegramClient::new());
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(18081, dir.path());

    let bot = TelerustBot::new(
        config,
        "test-token".to_string(),
        TokenSource::EnvVar,
        mock.clone(),
    );
    let handle = bot.shutdown_handle();
    tokio::spawn(async move {
        bot.run().await.unwrap();
    });

    // Wait for HTTP server to start
    tokio::time::sleep(Duration::from_millis(500)).await;

    let client = reqwest::Client::new();
    let resp = client
        .post("http://127.0.0.1:18081/send")
        .json(&serde_json::json!({"text": "hello from test"}))
        .send()
        .await
        .unwrap();

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["message_id"], 12345);

    let sent = mock.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, 42);
    assert_eq!(sent[0].1.text, "hello from test");

    handle.shutdown();
}

#[tokio::test]
async fn test_inbound_message_reaches_sse_subscriber() {
    let mock = Arc::new(MockTelegramClient::new());
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(18082, dir.path());

    let bot = TelerustBot::new(
        config,
        "test-token".to_string(),
        TokenSource::EnvVar,
        mock.clone(),
    );
    let handle = bot.shutdown_handle();
    tokio::spawn(async move {
        bot.run().await.unwrap();
    });

    // Wait for HTTP server and polling to start
    tokio::time::sleep(Duration::from_millis(500)).await;

    let client = reqwest::Client::new();
    let mut resp = client
        .get("http://127.0.0.1:18082/events")
        .send()
        .await
        .unwrap();

    // Brief pause to let SSE connection establish
    tokio::time::sleep(Duration::from_millis(100)).await;

    mock.simulate_inbound(InboundMessage {
        message_id: 777,
        text: "hello from telegram".to_string(),
        date: 1713400000,
        from_username: Some("testuser".to_string()),
    });

    let chunk = tokio::time::timeout(Duration::from_secs(3), resp.chunk())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    let text = String::from_utf8_lossy(&chunk);
    assert!(text.contains("hello from telegram"));
    assert!(text.contains("777"));

    handle.shutdown();
}
