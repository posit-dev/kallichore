//
// test_utils.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//

//! Common test utilities and helpers for Kallichore server tests

#![allow(dead_code)]

use kallichore_api::models::{InterruptMode, NewSession, VarAction, VarActionType};
use kallichore_api::{ApiNoContext, NewSessionResponse};
use kcshared::jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader};
use kcshared::websocket_message::WebsocketMessage;
use serde_json;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use swagger::{AuthData, ContextBuilder, EmptyContext, XSpanIdString};
use tokio::sync::OnceCell;
use uuid::Uuid;

// Cache Python executable discovery to avoid repeated lookups
static PYTHON_EXECUTABLE: OnceCell<Option<String>> = OnceCell::const_new();
static IPYKERNEL_AVAILABLE: OnceCell<bool> = OnceCell::const_new();

pub type ClientContext = swagger::make_context_ty!(
    ContextBuilder,
    EmptyContext,
    Option<AuthData>,
    XSpanIdString
);

/// Get a cached Python executable, finding it once per test run
pub async fn get_python_executable() -> Option<String> {
    PYTHON_EXECUTABLE
        .get_or_init(find_python_executable)
        .await
        .clone()
}

/// Check if ipykernel is available, caching the result
pub async fn is_ipykernel_available() -> bool {
    *IPYKERNEL_AVAILABLE
        .get_or_init(|| async {
            if let Some(python_cmd) = get_python_executable().await {
                check_ipykernel_available(&python_cmd).await
            } else {
                false
            }
        })
        .await
}

/// Create a standard NewSession for testing with Python kernel
pub fn create_test_session(session_id: String, python_cmd: &str) -> NewSession {
    NewSession {
        session_id,
        display_name: "Test Python Kernel".to_string(),
        language: "python".to_string(),
        username: "testuser".to_string(),
        input_prompt: "In [{}]: ".to_string(),
        continuation_prompt: "   ...: ".to_string(),
        argv: vec![
            python_cmd.to_string(),
            "-m".to_string(),
            "ipykernel".to_string(), // Use ipykernel instead of ipykernel_launcher
            "-f".to_string(),
            "{connection_file}".to_string(),
        ],
        working_directory: std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string(),
        env: vec![VarAction {
            action: VarActionType::Replace,
            name: "TEST_VAR".to_string(),
            value: "test_value".to_string(),
        }],
        connection_timeout: Some(3),
        interrupt_mode: InterruptMode::Message,
        protocol_version: Some("5.3".to_string()),
        run_in_shell: Some(false),
    }
}

/// Create a kernel_info_request message
pub fn create_kernel_info_request() -> WebsocketMessage {
    WebsocketMessage::Jupyter(JupyterMessage {
        header: JupyterMessageHeader {
            msg_id: Uuid::new_v4().to_string(),
            msg_type: "kernel_info_request".to_string(),
        },
        parent_header: None,
        channel: JupyterChannel::Shell,
        content: serde_json::json!({}),
        metadata: serde_json::json!({}),
        buffers: vec![],
    })
}

/// Create an execute_request message with test code
pub fn create_execute_request() -> WebsocketMessage {
    WebsocketMessage::Jupyter(JupyterMessage {
        header: JupyterMessageHeader {
            msg_id: Uuid::new_v4().to_string(),
            msg_type: "execute_request".to_string(),
        },
        parent_header: None,
        channel: JupyterChannel::Shell,
        content: serde_json::json!({
            "code": "print('Hello from Kallichore test!')\nresult = 2 + 3\nprint(f'2 + 3 = {result}')",
            "silent": false,
            "store_history": true,
            "user_expressions": {},
            "allow_stdin": false,
            "stop_on_error": true
        }),
        metadata: serde_json::json!({}),
        buffers: vec![],
    })
}

/// Helper function to properly clean up a spawned server process
pub fn cleanup_spawned_server(mut child: Child) {
    println!("Cleaning up spawned server (PID: {})", child.id());

    if let Err(e) = child.kill() {
        println!("Warning: Failed to terminate spawned server process: {}", e);
    }

    match child.wait() {
        Ok(status) => {
            println!("Spawned server process terminated with status: {}", status);
        }
        Err(e) => {
            println!("Warning: Failed to wait for spawned server process: {}", e);
        }
    }
}

/// Create a server command for testing with common options
pub fn create_server_command(args: &[&str]) -> Command {
    let binary_path = std::env::current_dir()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/kcserver")
        .with_extension(if cfg!(windows) { "exe" } else { "" });

    let mut cmd = if binary_path.exists() {
        println!("Using pre-built binary at: {:?}", binary_path);
        let mut c = Command::new(&binary_path);
        c.args(args);
        c
    } else {
        println!("Pre-built binary not found, using cargo run");
        let mut c = Command::new("cargo");
        c.args(&["run", "--bin", "kcserver", "--"]);
        c.args(args);
        c
    };

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.env("RUST_LOG", "info");

    cmd
}

/// Wait for a connection file to be created with timeout
pub async fn wait_for_connection_file(
    connection_file_path: &std::path::Path,
    timeout_attempts: u32,
) -> Result<(), String> {
    let mut attempts = 0;
    while !connection_file_path.exists() && attempts < timeout_attempts {
        tokio::time::sleep(Duration::from_millis(100)).await;
        attempts += 1;
        if attempts % 20 == 0 {
            println!(
                "Still waiting for connection file after {} attempts...",
                attempts
            );
        }
    }

    if !connection_file_path.exists() {
        return Err("Connection file was not created within timeout".to_string());
    }

    Ok(())
}

/// Create a session and handle the response
pub async fn create_session_with_client(
    client: &Box<dyn ApiNoContext<ClientContext> + Send + Sync>,
    new_session: NewSession,
) -> String {
    let session_response = client
        .new_session(new_session.clone())
        .await
        .expect("Failed to create new session");

    match session_response {
        NewSessionResponse::TheSessionID(session_info) => {
            println!("Created session: {:?}", session_info);
            new_session.session_id
        }
        NewSessionResponse::Unauthorized => panic!("Unauthorized"),
        NewSessionResponse::InvalidRequest(err) => panic!("Invalid request: {:?}", err),
    }
}

/// Create a session request JSON string for the integration test
pub async fn create_session_request_json(session_id: &str, python_cmd: &str) -> Option<String> {
    let ipykernel_module = get_ipykernel_module(python_cmd).await?;

    Some(format!(
        r#"{{"session_id": "{}", "display_name": "Test Session", "language": "python", "username": "testuser", "input_prompt": "In [{{}}]: ", "continuation_prompt": "   ...: ", "argv": ["{}", "-m", "{}", "-f", "{{connection_file}}"], "working_directory": "/tmp", "env": [], "connection_timeout": 60, "interrupt_mode": "message", "protocol_version": "5.3", "run_in_shell": false}}"#,
        session_id, python_cmd, ipykernel_module
    ))
}

async fn find_python_executable() -> Option<String> {
    let candidates = if cfg!(windows) {
        vec!["python", "python3", "py"]
    } else {
        vec!["python3", "python"]
    };

    for candidate in candidates {
        match tokio::process::Command::new(candidate)
            .arg("--version")
            .output()
            .await
        {
            Ok(output) if output.status.success() => {
                println!("Found Python at: {}", candidate);

                let which_cmd = if cfg!(windows) { "where" } else { "which" };

                if let Ok(which_output) = tokio::process::Command::new(which_cmd)
                    .arg(candidate)
                    .output()
                    .await
                {
                    if which_output.status.success() {
                        let full_path = String::from_utf8_lossy(&which_output.stdout)
                            .lines()
                            .next()
                            .unwrap_or(candidate)
                            .to_string();

                        // Check if this Python has ipykernel available
                        if check_ipykernel_available(&full_path).await {
                            println!("Python at {} has ipykernel - using it", full_path);
                            return Some(full_path);
                        } else {
                            println!("Python at {} does not have ipykernel - skipping", full_path);
                            continue;
                        }
                    }
                }

                // Fallback: check the candidate directly (without full path)
                if check_ipykernel_available(candidate).await {
                    println!("Python at {} has ipykernel - using it", candidate);
                    return Some(candidate.to_string());
                } else {
                    println!("Python at {} does not have ipykernel - skipping", candidate);
                    continue;
                }
            }
            _ => continue,
        }
    }
    None
}

async fn check_ipykernel_available(python_cmd: &str) -> bool {
    match tokio::process::Command::new(python_cmd)
        .args(&["-c", "import ipykernel; print('ipykernel available')"])
        .output()
        .await
    {
        Ok(output) => {
            if output.status.success() {
                println!("ipykernel is available");
                true
            } else {
                println!(
                    "ipykernel check failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                false
            }
        }
        Err(e) => {
            println!("Failed to check ipykernel: {}", e);
            false
        }
    }
}

/// Get the correct ipykernel module name for the given Python executable
pub async fn get_ipykernel_module(python_cmd: &str) -> Option<String> {
    // Try ipykernel_launcher first (older installations)
    let launcher_check = tokio::process::Command::new(python_cmd)
        .args(&[
            "-c",
            "import ipykernel_launcher; print('launcher_available')",
        ])
        .output()
        .await;

    if let Ok(output) = launcher_check {
        if output.status.success() {
            return Some("ipykernel_launcher".to_string());
        }
    }

    // Try ipykernel module directly (newer installations)
    let kernel_check = tokio::process::Command::new(python_cmd)
        .args(&["-c", "import ipykernel; print('kernel_available')"])
        .output()
        .await;

    if let Ok(output) = kernel_check {
        if output.status.success() {
            return Some("ipykernel".to_string());
        }
    }

    None
}
