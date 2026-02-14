// hello world
use crossbeam_channel::{unbounded, Receiver};
use serde::Deserialize;
use std::thread;
use std::time::Duration;
use tungstenite::connect;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    #[serde(rename = "session_start")]
    SessionStart {
        session_id: String,
        cwd: String,
        model: String,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        session_id: String,
        tool_name: String,
        file_path: String,
    },
}

pub struct WsClientHandle {
    pub _thread: thread::JoinHandle<()>,
}

pub fn start_ws_client() -> (Receiver<AgentEvent>, WsClientHandle) {
    let (tx, rx) = unbounded::<AgentEvent>();

    let handle = thread::spawn(move || {
        let url = "ws://127.0.0.1:8080/ws";
        loop {
            println!("[ws_client] Connecting to {}...", url);
            match connect(url) {
                Ok((mut socket, _response)) => {
                    println!("[ws_client] Connected!");
                    loop {
                        match socket.read() {
                            Ok(msg) => {
                                if msg.is_text() {
                                    let text = msg.into_text().unwrap_or_default();
                                    match serde_json::from_str::<AgentEvent>(&text) {
                                        Ok(event) => {
                                            let _ = tx.send(event);
                                        }
                                        Err(e) => {
                                            eprintln!("[ws_client] Failed to parse: {}", e);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("[ws_client] Read error: {}", e);
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[ws_client] Connection failed: {}", e);
                }
            }
            println!("[ws_client] Reconnecting in 2s...");
            thread::sleep(Duration::from_secs(2));
        }
    });

    let ws_handle = WsClientHandle { _thread: handle };
    (rx, ws_handle)
}
