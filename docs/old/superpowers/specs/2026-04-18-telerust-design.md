# Telerust Design Spec

**Date:** 2026-04-18
**Status:** Approved

## Overview

Telerust is a Rust crate and CLI that acts as a personal Telegram relay bot. It pairs with a single Telegram user and provides bidirectional messaging: local applications can send messages to the paired user via IPC (HTTP or Unix socket), and incoming messages from the paired user are streamed to local subscribers via SSE/NDJSON.

## Architecture

**Approach:** Monolithic async core. A single `TelerustBot` struct owns all state (`Arc<BotState>`) and spawns all tasks (polling, IPC servers, message buffer) onto the tokio runtime. One `run()` method starts everything and returns a `ShutdownHandle`. The only trait abstraction is `TelegramClient` at the external API boundary, used for testing.

**Runtime:** Tokio multi-threaded.

## Crate Structure

Single crate with both `[lib]` and `[[bin]]` targets.

```
telerust/
  Cargo.toml
  src/
    lib.rs            # public API: TelerustBot, ShutdownHandle, config re-exports
    config.rs         # TOML load/save, XDG path resolution, validation
    secret.rs         # three-tier token resolution: keyring > env > config
    telegram.rs       # TelegramClient trait + teloxide impl, send/poll logic
    pairing.rs        # paired user validation
    message.rs        # InboundMessage type, broadcast-based message buffer
    ipc/
      mod.rs          # shared types (IpcRequest, IpcResponse)
      http_server.rs  # axum on 127.0.0.1, POST /send + GET /events (SSE)
      unix_server.rs  # tokio UnixListener, two socket files
    daemon.rs         # daemonize logic, PID file write/read/signal
    cli.rs            # clap subcommands
  src/bin/
    telerust.rs       # thin main() that calls cli::run()
  tests/
    integration.rs    # mock TelegramClient, test IPC -> Telegram flow
  .github/
    workflows/
      ci.yml
```

## Core Data Types

### Config (TOML at `~/.config/telerust/config.toml`)

```rust
struct Config {
    bot_token: Option<String>,    // last resort, warned about at startup
    use_keyring: bool,
    paired: PairedUser,
    ipc: IpcConfig,
}

struct PairedUser {
    username: Option<String>,
    user_id: Option<i64>,
}

struct IpcConfig {
    unix_socket_path: PathBuf,    // default: $XDG_RUNTIME_DIR/telerust.sock (send socket)
                                  // events socket derived as telerust-events.sock in same dir
    http_port: u16,               // default: 8080, always bound to 127.0.0.1
}
```

### Runtime State

```rust
struct BotState {
    config: Config,
    token: String,                // resolved at startup
    telegram: Box<dyn TelegramClient>,
    message_buffer: MessageBuffer,  // tokio::sync::broadcast for SSE subscribers
}
```

### IPC Protocol

**Send request (local -> telerust -> Telegram):**

```json
{
    "text": "message content",
    "parse_mode": "MarkdownV2",
    "reply_to_message_id": 12345
}
```

- `text` — required
- `parse_mode` — optional: `"MarkdownV2"`, `"HTML"`, or `"Plain"` (default)
- `reply_to_message_id` — optional

**Send response:**

```json
{ "status": "ok", "message_id": 67890 }
{ "status": "error", "error": "Bad Request: can't parse entities" }
```

**Inbound message (telerust -> subscribers):**

```json
{"message_id": 12345, "text": "Hello", "date": 1713400000, "from_username": "user"}
```

### IPC Transports

| Transport | Send | Subscribe |
|-----------|------|-----------|
| HTTP | `POST /send` -> JSON response | `GET /events` -> SSE stream (persistent) |
| Unix socket (send) | `telerust.sock`: connect, write JSON, read response, close | — |
| Unix socket (subscribe) | — | `telerust-events.sock`: connect, read NDJSON stream (persistent) |

Two separate Unix socket files to avoid framing complexity. Subscribe connections are persistent; send connections are one-shot.

## Token Resolution

Three-tier priority chain:

1. **Keyring** — `keyring` crate, service name `telerust`, current OS user. Requires Secret Service on Linux (GNOME Keyring / KWallet).
2. **Environment variable** — `TELERUST_BOT_TOKEN`. Standard for server/container deployments.
3. **Config file** — `bot_token` field in TOML. Startup warning logged: "Bot token stored in plain text config file. Consider using keyring or TELERUST_BOT_TOKEN environment variable."

If `use_keyring=true`, try keyring first, then env var, then config. If `use_keyring=false`, try env var, then config.

## Telegram Integration

### TelegramClient Trait

```rust
#[async_trait]
trait TelegramClient: Send + Sync {
    async fn send_message(&self, chat_id: i64, request: &IpcRequest) -> Result<i32>;
    async fn get_me(&self) -> Result<BotInfo>;
    async fn start_polling(&self, handler: InboundHandler) -> Result<()>;
}
```

`InboundHandler` is a `tokio::sync::broadcast::Sender<InboundMessage>` — the polling loop sends incoming messages directly to the broadcast channel rather than through a callback.

### Teloxide Implementation

- `Bot::new(token)` initialized once at startup
- `start_polling` uses `teloxide::dispatching::Dispatcher` with `Update::filter_message()`
- Reconnection: teloxide handles long-polling reconnection internally; wrapper adds exponential backoff (1s, 2s, 4s... capped at 60s) with jitter for persistent API errors

### Pairing Validation

- Check `user_id` first (authoritative), fall back to `username` if only that is configured
- Non-paired messages logged at `trace!` level only (silent in production)

## Message Buffer

Wraps `tokio::sync::broadcast` channel. Incoming Telegram messages are broadcast to all connected SSE/NDJSON subscribers. No history replay — subscribers only receive messages arriving after connection.

## Daemon Lifecycle

### `start` (default: daemonize)

1. Load config, resolve token (three-tier)
2. Validate token via `get_me()`
3. Fork via `daemonize` crate, write PID to `$XDG_RUNTIME_DIR/telerust.pid`
4. Redirect logs to `$XDG_STATE_HOME/telerust/telerust.log`
5. Spawn tasks: long-polling, HTTP server, both Unix socket listeners
6. SIGINT/SIGTERM via `tokio::signal` -> stop polling, close IPC, flush logs, exit

### `start --foreground`

Same but no fork. Logs to stdout with `tracing_subscriber` env filter (`RUST_LOG`).

## CLI Commands

| Command | Description |
|---------|-------------|
| `init` | Interactive setup: bot token, paired user, IPC defaults. Validates token via `get_me()`. Writes config. |
| `pair` | Prompt for username/user_id, update config, best-effort test message. |
| `start` | Daemonize by default. `--foreground` flag for foreground mode. |
| `send <text>` | Send message to paired user, print result, exit. |
| `status` | Print config, token source, process status, IPC paths. |
| `stop` | Read PID file, send SIGTERM, wait up to 5s, report result. |

## Error Handling

- **`anyhow`** for all error propagation (Fail Fast phase)
- `expect("reason")` only in startup code where failure is unrecoverable
- No `unwrap()` anywhere

## Logging

- `tracing` + `tracing-subscriber` with `EnvFilter`
- Default: `info` — startup, shutdown, message metadata (not content)
- `debug` — config loading, token source, IPC connections
- `trace` — non-paired messages discarded, raw payloads
- Daemon mode: log file at `$XDG_STATE_HOME/telerust/telerust.log`
- Foreground mode: stdout

## Dependencies

| Crate | Purpose |
|-------|---------|
| `tokio` | async runtime |
| `teloxide` | Telegram bot API |
| `clap` | CLI argument parsing |
| `serde` + `serde_json` | JSON serialization |
| `toml` | config file format |
| `axum` | HTTP IPC server |
| `tracing` + `tracing-subscriber` | structured logging |
| `anyhow` | error handling |
| `directories` | XDG path resolution |
| `keyring` | optional secure token storage |
| `daemonize` | process daemonization |
| `tokio-stream` | SSE streaming for axum |

## Testing

### Unit Tests

- `config.rs` — TOML round-trip, XDG path resolution, validation
- `pairing.rs` — user_id match, username match, no match
- `secret.rs` — token resolution fallback chain (mock env, skip keyring in CI)
- `ipc/mod.rs` — request/response serialization

### Integration Test

- Mock `TelegramClient` that records sent messages and simulates incoming
- HTTP `/send` -> mock captures outbound with correct parse_mode
- Simulated incoming -> SSE subscriber receives `InboundMessage`
- Non-paired user -> silently dropped, nothing broadcast

### CI

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`
- Linux only

## Security Notes

- IPC listens only on `127.0.0.1` (HTTP) and `$XDG_RUNTIME_DIR` (Unix sockets)
- No authentication on IPC — relies on OS-level access control
- Token in config file triggers startup warning recommending keyring or env var
- `accept_external` deferred to a future version
