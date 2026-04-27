use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    pub message_id: i64,
    pub text: String,
    pub date: i64,
    pub from_username: Option<String>,
}

pub struct MessageBuffer {
    sender: broadcast::Sender<InboundMessage>,
}

impl MessageBuffer {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn send(&self, msg: InboundMessage) -> Result<()> {
        let _ = self.sender.send(msg);
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<InboundMessage> {
        self.sender.subscribe()
    }

    pub fn sender(&self) -> broadcast::Sender<InboundMessage> {
        self.sender.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inbound_message_serialization() {
        let msg = InboundMessage {
            message_id: 123,
            text: "Hello".to_string(),
            date: 1713400000,
            from_username: Some("testuser".to_string()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""message_id":123"#));
        assert!(json.contains(r#""text":"Hello""#));
        let deserialized: InboundMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.message_id, 123);
    }

    #[tokio::test]
    async fn test_message_buffer_broadcast() {
        let buffer = MessageBuffer::new(16);
        let mut rx = buffer.subscribe();
        let msg = InboundMessage {
            message_id: 1,
            text: "test".to_string(),
            date: 1713400000,
            from_username: None,
        };
        buffer.send(msg.clone()).unwrap();
        let received = rx.recv().await.unwrap();
        assert_eq!(received.message_id, 1);
        assert_eq!(received.text, "test");
    }

    #[tokio::test]
    async fn test_message_buffer_multiple_subscribers() {
        let buffer = MessageBuffer::new(16);
        let mut rx1 = buffer.subscribe();
        let mut rx2 = buffer.subscribe();
        let msg = InboundMessage {
            message_id: 2,
            text: "broadcast".to_string(),
            date: 1713400000,
            from_username: None,
        };
        buffer.send(msg).unwrap();
        let r1 = rx1.recv().await.unwrap();
        let r2 = rx2.recv().await.unwrap();
        assert_eq!(r1.message_id, 2);
        assert_eq!(r2.message_id, 2);
    }
}
