use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;
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
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let log_filter = warp::any().map(move || log.clone());

    let session_start = warp::post()
        .and(warp::path("session-start"))
        .and(warp::body::json())
        .and(log_filter.clone())
        .map(
            |payload: SessionStartPayload, log: Arc<Mutex<Vec<String>>>| {
                let entry = format!(
                    "[SessionStart] session_id={}, cwd={}, model={}",
                    payload.session_id, payload.cwd, payload.model
                );
                println!("{}", entry);
                tokio::spawn(async move {
                    log.lock().await.push(entry);
                });
                warp::reply::with_status("OK", StatusCode::OK)
            },
        );

    let read_event = warp::post()
        .and(warp::path("read"))
        .and(warp::body::json())
        .and(log_filter.clone())
        .map(|payload: ToolUsePayload, log: Arc<Mutex<Vec<String>>>| {
            let entry = format!(
                "[Read] session_id={}, tool_name={}, file_path={}",
                payload.session_id, payload.tool_name, payload.tool_input.file_path
            );
            println!("{}", entry);
            tokio::spawn(async move {
                log.lock().await.push(entry);
            });
            warp::reply::with_status("OK", StatusCode::OK)
        });

    let write_event = warp::post()
        .and(warp::path("write"))
        .and(warp::body::json())
        .and(log_filter.clone())
        .map(|payload: ToolUsePayload, log: Arc<Mutex<Vec<String>>>| {
            let entry = format!(
                "[Write] session_id={}, tool_name={}, file_path={}",
                payload.session_id, payload.tool_name, payload.tool_input.file_path
            );
            println!("{}", entry);
            tokio::spawn(async move {
                log.lock().await.push(entry);
            });
            warp::reply::with_status("OK", StatusCode::OK)
        });

    let edit_event = warp::post()
        .and(warp::path("edit"))
        .and(warp::body::json())
        .and(log_filter)
        .map(|payload: ToolUsePayload, log: Arc<Mutex<Vec<String>>>| {
            let entry = format!(
                "[Edit] session_id={}, tool_name={}, file_path={}",
                payload.session_id, payload.tool_name, payload.tool_input.file_path
            );
            println!("{}", entry);
            tokio::spawn(async move {
                log.lock().await.push(entry);
            });
            warp::reply::with_status("OK", StatusCode::OK)
        });

    let routes = session_start.or(read_event).or(write_event).or(edit_event);

    println!("Server starting on http://127.0.0.1:8080");
    warp::serve(routes).run(([127, 0, 0, 1], 8080)).await;
}
