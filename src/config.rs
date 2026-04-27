use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PairedUser {
    pub username: Option<String>,
    pub user_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcConfig {
    #[serde(default = "default_unix_socket_path")]
    pub unix_socket_path: PathBuf,
    #[serde(default = "default_http_port")]
    pub http_port: u16,
}

impl Default for IpcConfig {
    fn default() -> Self {
        Self {
            unix_socket_path: default_unix_socket_path(),
            http_port: default_http_port(),
        }
    }
}

fn default_unix_socket_path() -> PathBuf {
    dirs_runtime_dir().join("telerust.sock")
}

fn default_http_port() -> u16 {
    8080
}

fn dirs_runtime_dir() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub bot_token: Option<String>,
    #[serde(default = "default_use_keyring")]
    pub use_keyring: bool,
    pub paired: PairedUser,
    pub ipc: IpcConfig,
}

fn default_use_keyring() -> bool {
    true
}

impl Config {
    pub fn default_path() -> PathBuf {
        directories::ProjectDirs::from("", "", "telerust")
            .map(|dirs| dirs.config_dir().join("config.toml"))
            .unwrap_or_else(|| PathBuf::from("telerust.toml"))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        Ok(())
    }

    pub fn events_socket_path(&self) -> PathBuf {
        let parent = self
            .ipc
            .unix_socket_path
            .parent()
            .unwrap_or(Path::new("/tmp"));
        parent.join("telerust-events.sock")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_roundtrip() {
        let config = Config {
            bot_token: Some("test-token".to_string()),
            use_keyring: false,
            paired: PairedUser {
                username: Some("testuser".to_string()),
                user_id: Some(12345),
            },
            ipc: IpcConfig {
                unix_socket_path: PathBuf::from("/tmp/telerust.sock"),
                http_port: 9090,
            },
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(deserialized.bot_token, Some("test-token".to_string()));
        assert!(!deserialized.use_keyring);
        assert_eq!(deserialized.paired.username, Some("testuser".to_string()));
        assert_eq!(deserialized.paired.user_id, Some(12345));
        assert_eq!(deserialized.ipc.http_port, 9090);
    }

    #[test]
    fn test_config_defaults() {
        let toml_str = "use_keyring = true\n\n[paired]\n\n[ipc]\n";
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.use_keyring);
        assert!(config.bot_token.is_none());
        assert!(config.paired.username.is_none());
        assert!(config.paired.user_id.is_none());
        assert_eq!(config.ipc.http_port, 8080);
        assert!(config
            .ipc
            .unix_socket_path
            .to_str()
            .unwrap()
            .contains("telerust.sock"));
    }

    #[test]
    fn test_config_file_load_and_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let config = Config {
            bot_token: None,
            use_keyring: true,
            paired: PairedUser {
                username: Some("alice".to_string()),
                user_id: None,
            },
            ipc: IpcConfig::default(),
        };
        config.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.paired.username, Some("alice".to_string()));
        assert!(loaded.use_keyring);
    }

    #[test]
    fn test_config_path_resolution() {
        let path = Config::default_path();
        assert!(path.to_str().unwrap().contains("telerust"));
        assert!(path.to_str().unwrap().contains("config.toml"));
    }
}
