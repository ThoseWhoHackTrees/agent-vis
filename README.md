# 🌌 Space Agents!  - Real-time AI Agent Visualization

A beautiful 3D visualization tool that transforms your file system into an interactive galaxy, with AI agents represented as colorful spaceships flying between files as they work.


### Team Memebers: Alexandra Duan, Aanya Agrawal, Hanna Abrahem, Edward Wibowo 

## ✨ Features

### 🎨 Immersive 3D Galaxy Visualization
- **File System as Cosmos**: Every file and directory is rendered as a glowing star in a spiral galaxy formation
- **Color-coded File Types**: Instant visual identification
  - Rust files: Pastel coral
  - Config files: Pastel yellow
  - Documentation: Pastel lavender
  - JavaScript/TypeScript: Cream yellow
  - Python: Sky blue
  - And more!
- **Dynamic Bloom Effects**: Stars and spaceships glow with realistic light bloom
- **Orbital Rings**: Subtle animated orbit circles for depth
- **Ambient Stars**: Hundreds of color-shifting background stars

### 🚀 AI Agent Tracking
- **Spaceship Agents**: Each AI agent is represented as a 3D spaceship model
- **Unique Colors**: Every agent gets a persistent, unique color for easy tracking
- **Real-time Movement**: Watch agents fly smoothly between files they're working on
- **Activity Display**: See exactly what each agent is doing:
  - α Reading main.rs
  - β Writing config.toml
  - γ Editing agent.rs
- **Greek Symbol Identification**: Agents are labeled with Greek letters (α, β, γ, etc.)

### 📊 Interactive Dashboards

**Agent Activity Panel** (Top Left)
- Real-time list of active agents and their current actions
- Color-matched to spaceship colors
- Responsive text sizing

**File Statistics** (Bottom Left)
- Top 6 most visited/edited files
- Edit count tracking
- Color-coded by file type

**Camera Controls** (Bottom Left)
- **Auto Mode**: Smooth orbital camera movement
- **Manual Mode**: Full keyboard control
  - Arrow Keys: Rotate and zoom
  - W/S: Adjust height
- **Follow Mode**: (Coming soon) Track specific agents

**File Hover Info** (Top Right)
- Hover over any star to see recent activity
- Shows last 3 tool uses with timestamps
- Color-coded by operation type (Read/Write/Edit)
- Smooth fade-in/fade-out animations

**Color Legend** (Bottom Right)
- Quick reference for file type colors

### 🔥 Advanced Visual Effects
- **Hover Glow**: Files pulse with light when hovered
- **File Highlighting**: Recently visited files glow brightly
- **Smooth Animations**: Easing functions for all movements
- **Responsive Layout**: UI scales with window size
- **Fade Animations**: Elegant transitions for all panels

## 🏗️ Architecture

```
agent-vis/
├── frontend/          # Bevy-powered 3D visualization
│   ├── src/
│   │   ├── main.rs          # Main app & UI systems
│   │   ├── agent.rs         # Agent behavior & tracking
│   │   ├── galaxy.rs        # Star rendering & positioning
│   │   ├── fs_model.rs      # File system modeling
│   │   ├── watcher.rs       # File system watcher
│   │   └── ws_client.rs     # WebSocket client
│   └── assets/
│       ├── low_poly_spaceships.glb  # 3D spaceship models
│       └── fonts/
└── server/            # WebSocket event server
    └── src/
        └── main.rs    # Event aggregation & broadcasting
```

## 🚀 Getting Started

### Prerequisites
- **Rust** (1.75+): Install from [rustup.rs](https://rustup.rs/)
- **Cargo**: Comes with Rust
- **Git**: For cloning the repository

### Installation

1. **Clone the repository**
   ```bash
   git clone <your-repo-url>
   cd agent-vis
   ```

2. **Install dependencies**
   ```bash
   # Frontend dependencies (Bevy, etc.)
   cd frontend
   cargo build --release

   # Server dependencies
   cd ../server
   cargo build --release
   ```

### Running the Visualization

#### Step 1: Start the Server
The server receives events from AI agents and broadcasts them to the visualization.

```bash
cd server
cargo run --release
```

The server will start on `http://127.0.0.1:8080`

#### Step 2: Start the Frontend
The frontend creates the 3D visualization.

```bash
cd frontend
cargo run --release -- /path/to/your/project
```

**Example:**
```bash
cargo run --release -- ~/projects/my-ai-project
```

**Note:** The path should point to the directory you want to visualize. The app will:
- Build a model of the file system
- Watch for file changes
- Connect to the WebSocket server to receive agent events

#### Step 3: Connect Your AI Agents
Configure your AI agents to send events to the server at `http://127.0.0.1:8080`

**Event Types:**

**Session Start** (when an agent begins work)
```bash
curl -X POST http://127.0.0.1:8080/session-start \
  -H "Content-Type: application/json" \
  -d '{
    "session_id": "agent-1",
    "cwd": "/path/to/project",
    "model": "claude-sonnet-4"
  }'
```

**Tool Use** (when an agent reads/writes/edits a file)
```bash
curl -X POST http://127.0.0.1:8080/read \
  -H "Content-Type: application/json" \
  -d '{
    "session_id": "agent-1",
    "tool_name": "Read",
    "tool_input": {
      "file_path": "/path/to/file.rs"
    },
    "timestamp": "2024-01-15T14:30:45Z"
  }'
```

**Supported endpoints:**
- `/session-start` - Agent starts working
- `/read` - Agent reads a file
- `/write` - Agent writes to a file
- `/edit` - Agent edits a file

## 🎮 Controls

### Camera Controls
- **Auto Mode** (default): Camera orbits automatically
- **Manual Mode**: Take control with keyboard
  - `↑/↓ Arrow Keys`: Zoom in/out
  - `←/→ Arrow Keys`: Rotate around galaxy
  - `W/S Keys`: Move camera up/down

### Mouse Interactions
- **Hover over stars**: View recent file activity
- **Click mode buttons**: Switch camera modes

## 🎨 Customization

### Adjust Visual Settings

**Change galaxy colors** (`frontend/src/galaxy.rs`):
```rust
pub fn calculate_star_color(node: &FileNode) -> Color {
    match extension {
        "rs" => Color::srgb(1.0, 0.75, 0.6),  // Your custom color
        // ...
    }
}
```

**Adjust camera speed** (`frontend/src/main.rs`):
```rust
controller.orbit_angle += time.delta_secs() * 0.1;  // Change multiplier
```

**Modify agent speed** (`frontend/src/agent.rs`):
```rust
const MOVE_SPEED: f32 = 1.2;  // Seconds per move between files
```

## 🔧 Development

### Run in Development Mode
```bash
# Server with auto-reload
cd server
cargo watch -x run

# Frontend with faster compile times
cd frontend
cargo run -- /path/to/project
```

### Debug Mode
Set `RUST_LOG` for detailed logging:
```bash
RUST_LOG=debug cargo run -- /path/to/project
```

## 📊 Performance Tips

1. **Large Projects**: The visualization handles thousands of files, but very large projects may impact performance
2. **Release Mode**: Always use `--release` for smooth 60 FPS
3. **GPU**: Requires a GPU with OpenGL 3.3+ or Vulkan support
4. **RAM**: ~500MB for typical projects

## 🐛 Troubleshooting

**Spaceships not appearing?**
- Make sure the server is running
- Check that agent events are being sent to the correct endpoint
- Verify the file paths in events match your watched directory

**Low FPS?**
- Use release mode: `cargo run --release`
- Close other GPU-intensive applications
- Reduce window size

**Files not showing up?**
- Check `.gitignore` - the visualization respects gitignore rules
- Ensure you have read permissions for the directory

**Connection errors?**
- Verify server is running on port 8080
- Check firewall settings

## 🎯 Use Cases

- **AI Development**: Visualize how AI agents navigate and modify codebases
- **Code Review**: See which files are most frequently edited
- **Team Collaboration**: Watch multiple agents work simultaneously
- **Debugging**: Identify files agents repeatedly access
- **Demonstrations**: Beautiful way to showcase AI agent capabilities

## 🏆 Hackathon Features

Built for **TreeHacks 2026** with focus on:
- ✅ Real-time visualization
- ✅ Beautiful, intuitive UI
- ✅ Smooth animations and effects
- ✅ Multi-agent support
- ✅ Interactive exploration
- ✅ Production-ready architecture

## 🚀 Future Enhancements

- [ ] Follow mode to track specific agents
- [ ] Replay mode to review past agent sessions
- [ ] Agent collaboration visualization (multiple agents on same file)
- [ ] Export session data to JSON
- [ ] VR support for immersive exploration
- [ ] Agent communication visualization
- [ ] File diff visualization
- [ ] Performance metrics dashboard

## 📝 Technical Stack

- **Frontend**: Rust + Bevy Game Engine
- **3D Graphics**: bevy_fontmesh, bevy_picking
- **Networking**: WebSocket via tungstenite
- **Server**: Rust + warp async framework
- **File Watching**: notify crate
- **Effects**: Custom bloom and glow systems

## 👥 Team

Built with ❤️ for TreeHacks 2026


**Made with Rust 🦀 and Bevy ✨**

*Watch your AI agents explore the galaxy of code!* 🌌🚀
