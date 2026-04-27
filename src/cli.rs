use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::{info, warn};

use crate::config::Config;
use crate::daemon;
use crate::ipc::IpcRequest;
use crate::secret;
use crate::telegram::{TelegramClient, TeloxideClient};
use crate::TelerustBot;

#[derive(Parser)]
#[command(name = "telerust", about = "Personal Telegram relay bot")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Interactive setup: bot token, paired user, IPC defaults
    Init,
    /// Set or update the paired Telegram user
    Pair,
    /// Start the bot
    Start {
        /// Run in foreground instead of daemonizing
        #[arg(long)]
        foreground: bool,
    },
    /// Send a message to the paired user
    Send {
        /// Message text to send
        text: String,
    },
    /// Show status: config, token source, process status, IPC paths
    Status,
    /// Stop a running daemon
    Stop,
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init => cmd_init().await,
        Command::Pair => cmd_pair().await,
        Command::Start { foreground } => cmd_start(foreground).await,
        Command::Send { text } => cmd_send(text).await,
        Command::Status => cmd_status().await,
        Command::Stop => cmd_stop(),
    }
}

async fn cmd_init() -> Result<()> {
    use std::io::{self, Write};

    println!("Telerust Setup");
    println!("==============\n");

    print!("Enter your Telegram bot token: ");
    io::stdout().flush()?;
    let mut token = String::new();
    io::stdin().read_line(&mut token)?;
    let token = token.trim().to_string();

    println!("Validating token...");
    let client = TeloxideClient::new(&token, Default::default());
    let bot_info = client.get_me().await.context("Invalid bot token")?;
    println!(
        "Bot validated: @{} (ID: {})",
        bot_info.username, bot_info.id
    );

    print!("Store token in system keyring? [Y/n]: ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    let use_keyring = !answer.trim().eq_ignore_ascii_case("n");

    let mut bot_token_config = None;
    if use_keyring {
        match secret::store_keyring_token(&token) {
            Ok(()) => println!("Token stored in keyring."),
            Err(e) => {
                warn!("Failed to store in keyring: {e}. Falling back to config file.");
                bot_token_config = Some(token.clone());
            }
        }
    } else {
        bot_token_config = Some(token);
    }

    print!("Paired Telegram username (without @, or blank to skip): ");
    io::stdout().flush()?;
    let mut username = String::new();
    io::stdin().read_line(&mut username)?;
    let username = username.trim();
    let username = if username.is_empty() {
        None
    } else {
        Some(username.to_string())
    };

    print!("Paired Telegram user ID (or blank to skip): ");
    io::stdout().flush()?;
    let mut uid = String::new();
    io::stdin().read_line(&mut uid)?;
    let user_id: Option<i64> = uid.trim().parse().ok();

    let config = Config {
        bot_token: bot_token_config,
        use_keyring,
        paired: crate::config::PairedUser { username, user_id },
        ipc: crate::config::IpcConfig::default(),
    };

    let path = Config::default_path();
    config.save(&path)?;
    println!("\nConfig saved to {}", path.display());
    println!("Run `telerust start` to launch the bot.");
    Ok(())
}

async fn cmd_pair() -> Result<()> {
    use std::io::{self, Write};

    let path = Config::default_path();
    let mut config = Config::load(&path).with_context(|| {
        format!(
            "No config found at {}. Run `telerust init` first.",
            path.display()
        )
    })?;

    print!("Paired Telegram username (without @, or blank to keep current): ");
    io::stdout().flush()?;
    let mut username = String::new();
    io::stdin().read_line(&mut username)?;
    let username = username.trim();
    if !username.is_empty() {
        config.paired.username = Some(username.to_string());
    }

    print!("Paired Telegram user ID (or blank to keep current): ");
    io::stdout().flush()?;
    let mut uid = String::new();
    io::stdin().read_line(&mut uid)?;
    if let Ok(id) = uid.trim().parse::<i64>() {
        config.paired.user_id = Some(id);
    }

    config.save(&path)?;
    println!("Paired user updated. Config saved to {}", path.display());

    if let Some(user_id) = config.paired.user_id {
        let (token, _) = secret::resolve_token(&config)?;
        let client = TeloxideClient::new(&token, config.paired.clone());
        match client
            .send_message(
                user_id,
                &IpcRequest {
                    text: "Telerust pairing test - you are now paired!".to_string(),
                    parse_mode: None,
                    reply_to_message_id: None,
                },
            )
            .await
        {
            Ok(_) => println!("Test message sent successfully."),
            Err(e) => warn!("Could not send test message: {e}"),
        }
    }
    Ok(())
}

async fn cmd_start(foreground: bool) -> Result<()> {
    let config = Config::load(&Config::default_path())
        .context("No config found. Run `telerust init` first.")?;

    let (token, source) = secret::resolve_token(&config)?;
    info!("Token resolved from {source:?}");

    let client = TeloxideClient::new(&token, config.paired.clone());
    let bot_info = client
        .get_me()
        .await
        .context("Failed to validate bot token")?;
    info!("Bot: @{} (ID: {})", bot_info.username, bot_info.id);

    if !foreground {
        daemon::daemonize()?;
        setup_file_logging()?;
    } else {
        setup_stdout_logging();
    }

    let bot = TelerustBot::new(config, token, source, Arc::new(client));

    let handle = bot.shutdown_handle();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("Received shutdown signal");
        handle.shutdown();
    });

    bot.run().await
}

async fn cmd_send(text: String) -> Result<()> {
    let config = Config::load(&Config::default_path())
        .context("No config found. Run `telerust init` first.")?;

    let (token, _) = secret::resolve_token(&config)?;
    let chat_id = config
        .paired
        .user_id
        .context("No paired user_id configured. Run `telerust pair` first.")?;

    let client = TeloxideClient::new(&token, config.paired.clone());
    let request = IpcRequest {
        text,
        parse_mode: None,
        reply_to_message_id: None,
    };

    match client.send_message(chat_id, &request).await {
        Ok(id) => println!("Message sent (ID: {id})"),
        Err(e) => eprintln!("Error: {e}"),
    }
    Ok(())
}

async fn cmd_status() -> Result<()> {
    let path = Config::default_path();
    println!("Config file: {}", path.display());

    match Config::load(&path) {
        Ok(config) => {
            println!("\nPaired user:");
            if let Some(ref u) = config.paired.username {
                println!("  Username: @{u}");
            }
            if let Some(id) = config.paired.user_id {
                println!("  User ID: {id}");
            }
            if config.paired.username.is_none() && config.paired.user_id.is_none() {
                println!("  (not configured)");
            }
            println!("\nIPC:");
            println!("  HTTP port: 127.0.0.1:{}", config.ipc.http_port);
            println!("  Send socket: {}", config.ipc.unix_socket_path.display());
            println!("  Events socket: {}", config.events_socket_path().display());

            match secret::resolve_token(&config) {
                Ok((_, source)) => println!("\nToken source: {source:?}"),
                Err(e) => println!("\nToken: NOT FOUND ({e})"),
            }
        }
        Err(_) => {
            println!("  (no config file found)");
        }
    }

    println!("\nDaemon:");
    match daemon::read_pid() {
        Ok(pid) => {
            if daemon::is_running(pid) {
                println!("  Running (PID: {pid})");
            } else {
                println!("  Not running (stale PID file for {pid})");
            }
        }
        Err(_) => println!("  Not running"),
    }
    Ok(())
}

fn cmd_stop() -> Result<()> {
    daemon::stop(5)
}

fn setup_stdout_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}

fn setup_file_logging() -> Result<()> {
    let log_path = daemon::log_path();
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("Failed to open log file: {}", log_path.display()))?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(file)
        .with_ansi(false)
        .init();
    Ok(())
}
