use serde::{Deserialize, Serialize};

pub mod http_server;
pub mod unix_server;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcRequest {
    pub text: String,
    #[serde(default)]
    pub parse_mode: Option<ParseMode>,
    #[serde(default)]
    pub reply_to_message_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParseMode {
    MarkdownV2,
    HTML,
    Plain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum IpcResponse {
    #[serde(rename = "ok")]
    Ok { message_id: i64 },
    #[serde(rename = "error")]
    Error { error: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipc_request_minimal() {
        let json = r#"{"text":"hello"}"#;
        let req: IpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.text, "hello");
        assert!(req.parse_mode.is_none());
        assert!(req.reply_to_message_id.is_none());
    }

    #[test]
    fn test_ipc_request_full() {
        let json = r#"{"text":"hello","parse_mode":"MarkdownV2","reply_to_message_id":123}"#;
        let req: IpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.text, "hello");
        assert!(matches!(req.parse_mode, Some(ParseMode::MarkdownV2)));
        assert_eq!(req.reply_to_message_id, Some(123));
    }

    #[test]
    fn test_ipc_response_ok() {
        let resp = IpcResponse::Ok { message_id: 456 };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""status":"ok""#));
        assert!(json.contains(r#""message_id":456"#));
    }

    #[test]
    fn test_ipc_response_error() {
        let resp = IpcResponse::Error {
            error: "bad request".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""status":"error""#));
        assert!(json.contains("bad request"));
    }
}
