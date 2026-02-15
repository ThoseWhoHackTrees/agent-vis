// hello world
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::broadcast;
use warp::ws::Message;
use warp::{Filter, http::StatusCode};

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

#[tokio::main]
async fn main() {
    let (tx, _rx) = broadcast::channel::<String>(256);

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

    // Spawn a task to forward broadcast messages to this WebSocket client
    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if ws_tx.send(Message::text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Drain incoming messages (we don't use them, but need to keep the connection alive)
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(_)) = ws_rx.next().await {}
    });

    // If either task ends, clean up
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}
