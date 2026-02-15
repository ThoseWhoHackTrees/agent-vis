use chrono::Utc;
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use ignore::WalkBuilder;
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::IndexedRandom;
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use warp::ws::Message;
use warp::{Filter, http::StatusCode};

#[derive(Parser)]
#[command(about = "Agent visualization server")]
struct Args {
    /// Send mock Claude events over WebSocket for testing.
    /// Provide a directory path to use real files from that path (respects .gitignore).
    #[arg(long)]
    mock: Option<PathBuf>,
}

#[derive(Deserialize, Debug)]
struct SessionStartPayload {
    session_id: String,
    cwd: String,
    model: String,
}

#[derive(Deserialize, Debug)]
struct ToolInput {
    file_path: String,
}

#[derive(Deserialize, Debug)]
struct ToolUsePayload {
    session_id: String,
    tool_name: String,
    tool_input: ToolInput,
    #[serde(default)]
    reason: Option<String>,
}

/// Collect all file paths under `root`, respecting .gitignore.
fn collect_files(root: &PathBuf) -> Vec<String> {
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.clone());
    let mut files = Vec::new();
    for entry in WalkBuilder::new(&canonical_root).build() {
        if let Ok(entry) = entry {
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                files.push(entry.path().to_string_lossy().to_string());
            }
        }
    }
    files
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let (tx, _rx) = broadcast::channel::<String>(256);

    if let Some(mock_path) = args.mock {
        let files = collect_files(&mock_path);
        if files.is_empty() {
            eprintln!(
                "[mock] No files found under {:?} (check the path and .gitignore)",
                mock_path
            );
            std::process::exit(1);
        }
        let cwd = mock_path
            .canonicalize()
            .unwrap_or(mock_path)
            .to_string_lossy()
            .to_string();
        println!("[mock] Mock mode enabled — {} files from {}", files.len(), cwd);
        let files = Arc::new(files);
        let mock_tx = tx.clone();
        tokio::spawn(run_mock_sessions(mock_tx, files, cwd));
    }

    let tx_filter = {
        let tx = tx.clone();
        warp::any().map(move || tx.clone())
    };

    let session_start = warp::post()
        .and(warp::path("session-start"))
        .and(warp::body::json())
        .and(tx_filter.clone())
        .map(|payload: SessionStartPayload, tx: broadcast::Sender<String>| {
            let msg = json!({
                "type": "session_start",
                "session_id": payload.session_id,
                "cwd": payload.cwd,
                "model": payload.model,
            })
            .to_string();
            println!("[SessionStart] {}", msg);
            let _ = tx.send(msg);
            warp::reply::with_status("OK", StatusCode::OK)
        });

    let read_event = warp::post()
        .and(warp::path("read"))
        .and(warp::body::json())
        .and(tx_filter.clone())
        .map(|payload: ToolUsePayload, tx: broadcast::Sender<String>| {
            let msg = json!({
                "type": "tool_use",
                "session_id": payload.session_id,
                "tool_name": payload.tool_name,
                "file_path": payload.tool_input.file_path,
                "reason": payload.reason,
                "timestamp": Utc::now().to_rfc3339(),
            })
            .to_string();
            println!("[Read] {}", msg);
            let _ = tx.send(msg);
            warp::reply::with_status("OK", StatusCode::OK)
        });

    let write_event = warp::post()
        .and(warp::path("write"))
        .and(warp::body::json())
        .and(tx_filter.clone())
        .map(|payload: ToolUsePayload, tx: broadcast::Sender<String>| {
            let msg = json!({
                "type": "tool_use",
                "session_id": payload.session_id,
                "tool_name": payload.tool_name,
                "file_path": payload.tool_input.file_path,
                "reason": payload.reason,
                "timestamp": Utc::now().to_rfc3339(),
            })
            .to_string();
            println!("[Write] {}", msg);
            let _ = tx.send(msg);
            warp::reply::with_status("OK", StatusCode::OK)
        });

    let edit_event = warp::post()
        .and(warp::path("edit"))
        .and(warp::body::json())
        .and(tx_filter)
        .map(|payload: ToolUsePayload, tx: broadcast::Sender<String>| {
            let msg = json!({
                "type": "tool_use",
                "session_id": payload.session_id,
                "tool_name": payload.tool_name,
                "file_path": payload.tool_input.file_path,
                "reason": payload.reason,
                "timestamp": Utc::now().to_rfc3339(),
            })
            .to_string();
            println!("[Edit] {}", msg);
            let _ = tx.send(msg);
            warp::reply::with_status("OK", StatusCode::OK)
        });

    let ws_route = {
        let tx = tx.clone();
        warp::path("ws")
            .and(warp::ws())
            .map(move |ws: warp::ws::Ws| {
                let rx = tx.subscribe();
                ws.on_upgrade(move |websocket| handle_ws_client(websocket, rx))
            })
    };

    let routes = session_start
        .or(read_event)
        .or(write_event)
        .or(edit_event)
        .or(ws_route);

    println!("Server starting on http://127.0.0.1:8080");
    warp::serve(routes).run(([127, 0, 0, 1], 8080)).await;
}

async fn handle_ws_client(websocket: warp::ws::WebSocket, mut rx: broadcast::Receiver<String>) {
    let (mut ws_tx, mut ws_rx) = websocket.split();

    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if ws_tx.send(Message::text(msg)).await.is_err() {
                break;
            }
        }
    });

    let recv_task = tokio::spawn(async move {
        while let Some(Ok(_)) = ws_rx.next().await {}
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}

/// Manages the lifecycle of multiple concurrent mock sessions.
async fn run_mock_sessions(
    tx: broadcast::Sender<String>,
    files: Arc<Vec<String>>,
    cwd: String,
) {
    tokio::time::sleep(Duration::from_secs(1)).await;

    let models = ["claude-sonnet-4-5-20250929", "claude-opus-4-6"];
    let mut rng = StdRng::from_os_rng();
    let mut session_counter: u32 = 0;

    // Keep 2–4 sessions alive concurrently, staggering their starts.
    let max_concurrent = 2 + (rng.random::<u32>() % 3); // 2–4
    let mut handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();

    loop {
        // Clean up finished sessions
        handles.retain(|h| !h.is_finished());

        // Spawn new sessions up to the concurrent limit
        while (handles.len() as u32) < max_concurrent {
            session_counter += 1;
            let session_id = format!("mock-session-{}", session_counter);
            let model = *models.choose(&mut rng).unwrap();
            let tx = tx.clone();
            let files = Arc::clone(&files);
            let cwd = cwd.clone();
            // Stagger the initial delay per session so they don't all fire at once
            let initial_delay = rng.random_range(0..2000u64);

            handles.push(tokio::spawn(run_single_session(
                tx,
                files,
                cwd,
                session_id,
                model.to_string(),
                initial_delay,
            )));
        }

        // Check back periodically to see if we need to replace finished sessions
        let check_interval = 1000 + (rng.random::<u64>() % 3000);
        tokio::time::sleep(Duration::from_millis(check_interval)).await;
    }
}

/// Generate a human-readable explanation for a tool use action
fn generate_action_explanation(tool_name: &str, file_path: &str, action_number: u32, total_actions: u32) -> String {
    use std::path::Path;

    let file_name = Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");

    let ext = Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    // Phase-based explanations
    let phase_ratio = action_number as f32 / total_actions as f32;

    match tool_name {
        "Read" => {
            if phase_ratio < 0.3 {
                // Early exploration phase
                vec![
                    format!("Reading {} to understand the codebase structure", file_name),
                    format!("Scanning {} to identify dependencies", file_name),
                    format!("Reviewing {} to understand the current implementation", file_name),
                    format!("Checking {} for existing patterns and conventions", file_name),
                    format!("Examining {} to locate the entry point", file_name),
                ].choose(&mut StdRng::from_os_rng()).unwrap().clone()
            } else {
                // Later investigation phase
                vec![
                    format!("Reading {} to verify the changes needed", file_name),
                    format!("Checking {} before making modifications", file_name),
                    format!("Reviewing {} to ensure compatibility", file_name),
                    format!("Analyzing {} to understand the impact area", file_name),
                ].choose(&mut StdRng::from_os_rng()).unwrap().clone()
            }
        },
        "Write" => {
            if ext == "rs" {
                vec![
                    format!("Writing {} to add new functionality", file_name),
                    format!("Creating {} with the required implementation", file_name),
                    format!("Writing {} to introduce the new module", file_name),
                ].choose(&mut StdRng::from_os_rng()).unwrap().clone()
            } else {
                vec![
                    format!("Writing {} to update configuration", file_name),
                    format!("Creating {} with new settings", file_name),
                    format!("Writing {} to document the changes", file_name),
                ].choose(&mut StdRng::from_os_rng()).unwrap().clone()
            }
        },
        "Edit" => {
            if ext == "rs" {
                vec![
                    format!("Editing {} to fix the identified issue", file_name),
                    format!("Updating {} to improve the implementation", file_name),
                    format!("Modifying {} to add the requested feature", file_name),
                    format!("Refactoring {} to follow best practices", file_name),
                    format!("Editing {} to integrate the new functionality", file_name),
                ].choose(&mut StdRng::from_os_rng()).unwrap().clone()
            } else {
                vec![
                    format!("Editing {} to update configuration", file_name),
                    format!("Updating {} to fix inconsistencies", file_name),
                    format!("Modifying {} to align with requirements", file_name),
                ].choose(&mut StdRng::from_os_rng()).unwrap().clone()
            }
        },
        _ => format!("{} {}", tool_name, file_name),
    }
}

/// Simulates a single agent session: start → several tool uses → end.
async fn run_single_session(
    tx: broadcast::Sender<String>,
    files: Arc<Vec<String>>,
    cwd: String,
    session_id: String,
    model: String,
    initial_delay: u64,
) {
    let mut rng = StdRng::from_os_rng();
    let tool_names = ["Read", "Write", "Edit"];
    // Realistic timing: thinking pauses + tool execution
    let short_delays: [u64; 4] = [200, 400, 600, 900];
    let long_delays: [u64; 4] = [1500, 2500, 4000, 6000];

    tokio::time::sleep(Duration::from_millis(initial_delay)).await;

    // Session start
    let start_msg = json!({
        "type": "session_start",
        "session_id": session_id,
        "cwd": cwd,
        "model": model,
    })
    .to_string();
    println!("[mock] {}", start_msg);
    let _ = tx.send(start_msg);

    // Simulate a realistic work pattern: read several files, then edit/write a few.
    // Total actions: 4–12
    let num_actions = 4 + (rng.random::<u32>() % 9);

    for i in 0..num_actions {
        // Bias toward reads early in the session, writes/edits later (realistic agent behavior)
        let tool = if i < num_actions / 3 {
            // Early phase: mostly reads
            if rng.random::<f32>() < 0.85 {
                "Read"
            } else {
                *tool_names.choose(&mut rng).unwrap()
            }
        } else {
            // Later phase: mix of all tools
            *tool_names.choose(&mut rng).unwrap()
        };

        let path = files.choose(&mut rng).unwrap();

        // Generate explanation for this action
        let explanation = generate_action_explanation(tool, path, i, num_actions);

        // Occasionally have a "thinking" pause (longer delay), otherwise quick succession
        let delay = if rng.random::<f32>() < 0.3 {
            *long_delays.choose(&mut rng).unwrap()
        } else {
            *short_delays.choose(&mut rng).unwrap()
        };
        tokio::time::sleep(Duration::from_millis(delay)).await;

        let tool_msg = json!({
            "type": "tool_use",
            "session_id": session_id,
            "tool_name": tool,
            "file_path": path,
            "reason": explanation,
            "timestamp": Utc::now().to_rfc3339(),
        })
        .to_string();
        println!("[mock] {}", tool_msg);
        let _ = tx.send(tool_msg);
    }

    // Session lives for a bit after last action before "finishing"
    let wind_down = 1000 + (rng.random::<u64>() % 3000);
    tokio::time::sleep(Duration::from_millis(wind_down)).await;
}
