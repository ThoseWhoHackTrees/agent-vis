# Agent Event Server

A simple HTTP server that receives Claude hook events.

## Endpoints

- `POST /session-start` - Receives SessionStart events (cwd, model)
- `POST /read` - Receives Read tool events (tool_name, file_path)
- `POST /write` - Receives Write tool events (tool_name, file_path)
- `POST /edit` - Receives Edit tool events (tool_name, file_path)

## Running

```bash
cd server
cargo run
```

Server runs on http://127.0.0.1:8080

## Building Release

```bash
cd server
cargo build --release
```

## Hook Configuration

Hooks are configured in `hooks/settings.json` and `.claude/settings.json`.

The `hooks/log_stdin.sh` script receives hook events and forwards them to the server.
