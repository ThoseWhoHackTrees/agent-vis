# Space Agents!

A 3D command center for agentic swarms. Renders your codebase as a galaxy and your AI agents as spaceships navigating it in real time.

![alt text](<Demo.png>)
Built at TreeHacks 2026!

## What it does

Space Agents! visualizes a live codebase as a spiral galaxy: files and directories become stars, and each AI agent is a spaceship flying between them. View what every agent is doing (reading, writing, editing), which files are hot, and how your project is structured at a glance.

Files are color-coded by type and scaled by size. Agents are labeled and color-matched for tracking. Hover over any star to see recent activity. Zoom, orbit, or let the camera fly on autopilot.

## Architecture

```
├── frontend/          # Bevy 3D visualization
│   └── src/
│       ├── main.rs        # App entry, UI systems
│       ├── agent.rs       # Agent tracking & movement
│       ├── galaxy.rs      # Star rendering & layout
│       ├── fs_model.rs    # File system model
│       ├── watcher.rs     # FS watcher (notify crate)
│       └── ws_client.rs   # WebSocket client
└── server/            # Event relay server
    └── src/
        └── main.rs        # Ingests agent events, broadcasts via WS
```

Claude Code hooks stream agent telemetry to the server, which relays it to the frontend over WebSockets. The frontend watches the filesystem directly via the Rust `notify` crate and renders everything with the Bevy game engine.

## Stack

- **Frontend:** Rust + Bevy
- **Server:** Rust + warp
- **Networking:** WebSocket (tungstenite)
- **FS watching:** notify crate
- **Rendering:** Custom bloom/glow, bevy_picking, bevy_fontmesh

## Getting started

### Prerequisites

- Rust 1.75+ ([rustup.rs](https://rustup.rs/))

### Build

```bash
git clone <your-repo-url>
cd agent-vis

# build both
cd frontend && cargo build --release
cd ../server && cargo build --release
```

### Run

**1. Start the server**

```bash
cd server
cargo run --release
# listens on http://127.0.0.1:8080
```

**2. Start the frontend**

```bash
cd frontend
cargo run --release -- /path/to/your/project
```

This will model the file tree, watch for changes, and connect to the server for agent events.

**3. Connect agents**

Point your AI agents at `http://127.0.0.1:8080`. Supported endpoints:

- `POST /session-start` — agent begins a session
- `POST /read` — agent reads a file
- `POST /write` — agent writes a file
- `POST /edit` — agent edits a file

Example:

```bash
curl -X POST http://127.0.0.1:8080/session-start \
  -H "Content-Type: application/json" \
  -d '{"session_id": "agent-1", "cwd": "/path/to/project", "model": "claude-sonnet-4"}'
```

## Controls

- **Auto mode** (default): camera orbits on its own
- **Manual mode**: arrow keys to rotate/zoom, W/S to adjust height
- **Hover** over any star to see recent file activity

## Development

```bash
# server with auto-reload
cd server && cargo watch -x run

# frontend (debug build, faster compiles)
cd frontend && cargo run -- /path/to/project

# verbose logging
RUST_LOG=debug cargo run -- /path/to/project
```

## Notes

- Always use `--release` for smooth performance
- Requires GPU with OpenGL 3.3+ or Vulkan
- Respects `.gitignore` when building the file tree
