use anyhow::{bail, Result};
use tracing::{debug, warn};

use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSource {
    Keyring,
    EnvVar,
    ConfigFile,
}

pub fn resolve_token(config: &Config) -> Result<(String, TokenSource)> {
    resolve_token_inner(config, config.use_keyring)
}

fn resolve_token_inner(config: &Config, try_keyring: bool) -> Result<(String, TokenSource)> {
    if try_keyring {
        match try_keyring_token() {
            Ok(token) => {
                debug!("Token resolved from keyring");
                return Ok((token, TokenSource::Keyring));
            }
            Err(e) => {
                debug!("Keyring lookup failed: {e}");
            }
        }
    }

    if let Ok(token) = std::env::var("TELERUST_BOT_TOKEN") {
        if !token.is_empty() {
            debug!("Token resolved from TELERUST_BOT_TOKEN environment variable");
            return Ok((token, TokenSource::EnvVar));
        }
    }

    if let Some(ref token) = config.bot_token {
        if !token.is_empty() {
            warn!(
                "Bot token stored in plain text config file. \
                 Consider using keyring or TELERUST_BOT_TOKEN environment variable."
            );
            return Ok((token.clone(), TokenSource::ConfigFile));
        }
    }

    bail!(
        "No bot token found. Set TELERUST_BOT_TOKEN, use `telerust init` to configure, \
         or add bot_token to config file."
    )
}

fn try_keyring_token() -> Result<String> {
    let user = whoami::username().map_err(|e| anyhow::anyhow!("Failed to get username: {e}"))?;
    let entry = keyring::Entry::new("telerust", &user)?;
    let password = entry.get_password()?;
    Ok(password)
}

pub fn store_keyring_token(token: &str) -> Result<()> {
    let user = whoami::username().map_err(|e| anyhow::anyhow!("Failed to get username: {e}"))?;
    let entry = keyring::Entry::new("telerust", &user)?;
    entry.set_password(token)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, IpcConfig};
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn make_config(token: Option<&str>, use_keyring: bool) -> Config {
        Config {
            bot_token: token.map(String::from),
            use_keyring,
            paired: Default::default(),
            ipc: IpcConfig::default(),
        }
    }

    #[test]
    fn test_env_var_takes_priority_over_config() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("TELERUST_BOT_TOKEN", "env-token");
        let config = make_config(Some("config-token"), false);
        let result = resolve_token_inner(&config, false);
        std::env::remove_var("TELERUST_BOT_TOKEN");
        let (token, source) = result.unwrap();
        assert_eq!(token, "env-token");
        assert_eq!(source, TokenSource::EnvVar);
    }

    #[test]
    fn test_config_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("TELERUST_BOT_TOKEN");
        let config = make_config(Some("config-token"), false);
        let result = resolve_token_inner(&config, false);
        let (token, source) = result.unwrap();
        assert_eq!(token, "config-token");
        assert_eq!(source, TokenSource::ConfigFile);
    }

    #[test]
    fn test_no_token_available() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("TELERUST_BOT_TOKEN");
        let config = make_config(None, false);
        let result = resolve_token_inner(&config, false);
        assert!(result.is_err());
    }
}
