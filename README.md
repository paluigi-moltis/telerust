# Telerust

Personal Telegram relay bot. Pairs with a single Telegram user and provides bidirectional messaging: local applications send messages via IPC (HTTP or Unix socket), and incoming Telegram messages are streamed to local subscribers via SSE/NDJSON.

## Setup

```bash
cargo build --release
./target/release/telerust init
```

`init` walks you through bot token entry, paired user configuration, and validates the token against Telegram.

## Usage

```bash
# Start as daemon
telerust start

# Start in foreground (logs to stdout)
telerust start --foreground

# Send a message
telerust send "Hello from the CLI"

# Check status
telerust status

# Stop the daemon
telerust stop
```

## IPC

### HTTP (127.0.0.1:8080)

Send a message:
```bash
curl -X POST http://127.0.0.1:8080/send \
  -H "Content-Type: application/json" \
  -d '{"text": "hello"}'
```

Subscribe to inbound messages (SSE):
```bash
curl -N http://127.0.0.1:8080/events
```

### Unix Sockets

Send via socket:
```bash
echo '{"text":"hello"}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/telerust.sock
```

Subscribe to events (NDJSON):
```bash
socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/telerust-events.sock
```

## Token Storage

Priority: system keyring > `TELERUST_BOT_TOKEN` env var > config file (warns on startup).

## Configuration

Default config: `~/.config/telerust/config.toml`

```toml
use_keyring = true

[paired]
username = "your_telegram_username"
user_id = 123456789

[ipc]
http_port = 8080
```

## Development

```bash
cargo fmt
cargo clippy -- -D warnings
cargo test
```

## License

MIT
