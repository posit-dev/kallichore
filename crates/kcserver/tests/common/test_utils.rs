//
// test_utils.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Common test utilities and helpers for Kallichore server tests

#![allow(dead_code)]

use kallichore_api::models::{InterruptMode, NewSession, SessionMode, VarAction, VarActionType};
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
        notebook_uri: None,
        session_mode: SessionMode::Console,
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
        connection_timeout: Some(10),
        interrupt_mode: InterruptMode::Message,
        protocol_version: Some("5.3".to_string()),
        startup_environment: kallichore_api::models::StartupEnvironment::None,
        startup_environment_arg: None,
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

/// Create a shutdown_request message
pub fn create_shutdown_request() -> WebsocketMessage {
    WebsocketMessage::Jupyter(JupyterMessage {
        header: JupyterMessageHeader {
            msg_id: Uuid::new_v4().to_string(),
            msg_type: "shutdown_request".to_string(),
        },
        parent_header: None,
        channel: JupyterChannel::Shell,
        content: serde_json::json!({
            "restart": false
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

/// The connection details the server reports over the handshake socket. Mirrors
/// the `HandshakePayload` struct on the server side.
#[derive(serde::Deserialize, Debug, Clone)]
#[allow(dead_code)]
pub struct HandshakeConnectionInfo {
    pub port: Option<u16>,
    pub base_path: Option<String>,
    pub socket_path: Option<String>,
    pub named_pipe: Option<String>,
    pub transport: String,
    pub server_path: String,
    pub server_pid: u32,
    pub bearer_token: Option<String>,
    pub log_path: Option<String>,
    pub server_id: String,
}

/// A client-owned handshake endpoint that the server connects to once at
/// startup to report its connection details. The test creates and listens on
/// the endpoint first, passes `path()` to the server via `--handshake-socket`,
/// then awaits the single JSON payload with `recv()`.
pub struct HandshakeListener {
    path: String,
    #[cfg(unix)]
    listener: tokio::net::UnixListener,
    #[cfg(windows)]
    server: tokio::net::windows::named_pipe::NamedPipeServer,
}

impl HandshakeListener {
    /// Create and start listening on a fresh handshake endpoint.
    pub async fn create() -> Self {
        #[cfg(unix)]
        {
            let path = std::env::temp_dir()
                .join(format!("kc-handshake-{}.sock", Uuid::new_v4()))
                .to_string_lossy()
                .to_string();
            let listener =
                tokio::net::UnixListener::bind(&path).expect("Failed to bind handshake socket");
            HandshakeListener { path, listener }
        }

        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ServerOptions;
            let path = format!(r"\\.\pipe\kc-handshake-{}", Uuid::new_v4().simple());
            let server = ServerOptions::new()
                .first_pipe_instance(true)
                .create(&path)
                .expect("Failed to create handshake named pipe");
            HandshakeListener { path, server }
        }
    }

    /// The path/name to pass to the server via `--handshake-socket`.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Await the single JSON payload the server writes, reading to EOF, and
    /// return the parsed connection info. Fails if nothing arrives in time.
    pub async fn recv(self) -> Result<HandshakeConnectionInfo, String> {
        let read_fut = self.read_payload();
        match tokio::time::timeout(Duration::from_secs(30), read_fut).await {
            Ok(Ok(info)) => Ok(info),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("Timed out waiting for handshake payload".to_string()),
        }
    }

    #[cfg(unix)]
    async fn read_payload(self) -> Result<HandshakeConnectionInfo, String> {
        use tokio::io::AsyncReadExt;
        let (mut stream, _) = self
            .listener
            .accept()
            .await
            .map_err(|e| format!("Failed to accept handshake connection: {}", e))?;
        let mut buf = Vec::new();
        stream
            .read_to_end(&mut buf)
            .await
            .map_err(|e| format!("Failed to read handshake payload: {}", e))?;
        serde_json::from_slice(&buf)
            .map_err(|e| format!("Failed to parse handshake payload: {}", e))
    }

    #[cfg(windows)]
    async fn read_payload(mut self) -> Result<HandshakeConnectionInfo, String> {
        use tokio::io::AsyncReadExt;
        self.server
            .connect()
            .await
            .map_err(|e| format!("Failed to accept handshake connection: {}", e))?;
        let mut buf = Vec::new();
        self.server
            .read_to_end(&mut buf)
            .await
            .map_err(|e| format!("Failed to read handshake payload: {}", e))?;
        serde_json::from_slice(&buf)
            .map_err(|e| format!("Failed to parse handshake payload: {}", e))
    }
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
        r#"{{"session_id": "{}", "display_name": "Test Session", "language": "python", "username": "testuser", "input_prompt": "In [{{}}]: ", "continuation_prompt": "   ...: ", "argv": ["{}", "-m", "{}", "-f", "{{connection_file}}"], "working_directory": "/tmp", "env": [], "connection_timeout": 60, "interrupt_mode": "message", "protocol_version": "5.3", "startup_environment": "none"}}"#,
        session_id, python_cmd, ipykernel_module
    ))
}

async fn find_python_executable() -> Option<String> {
    let candidates = if cfg!(windows) {
        vec!["python", "python3", "py"]
    } else {
        vec!["python3", "python"]
    };

    // First try standard Python commands in PATH
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

    // On Windows, if not found in PATH, try common installation locations
    #[cfg(windows)]
    {
        println!("Python not found in PATH, checking common Windows installation locations...");

        let home_dir = std::env::var("USERPROFILE").unwrap_or_default();
        if !home_dir.is_empty() {
            let mut potential_paths = vec![
                // pyenv-win
                format!("{}\\.pyenv\\pyenv-win\\shims\\python.bat", home_dir),
            ];

            // Standard Python.org installations for multiple versions
            for version in &["313", "312", "311", "310", "39", "38"] {
                potential_paths.push(format!(
                    "{}\\AppData\\Local\\Programs\\Python\\Python{}\\python.exe",
                    home_dir, version
                ));
            }

            // Also check C:\Python directories (sometimes used in CI environments)
            for version in &["313", "312", "311", "310", "39", "38"] {
                potential_paths.push(format!("C:\\Python{}\\python.exe", version));
            }

            // Check Microsoft Store installations
            potential_paths.push(format!(
                "{}\\AppData\\Local\\Microsoft\\WindowsApps\\python.exe",
                home_dir
            ));

            for path in potential_paths {
                if std::path::Path::new(&path).exists() {
                    println!("Checking Python installation at: {}", path);
                    if check_ipykernel_available(&path).await {
                        println!("Found Python with ipykernel at: {}", path);
                        return Some(path);
                    } else {
                        println!("Python at {} does not have ipykernel - skipping", path);
                    }
                }
            }
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
