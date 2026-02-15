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
