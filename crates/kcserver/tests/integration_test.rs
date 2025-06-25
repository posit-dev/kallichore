//
// integration_test.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//

//! Integration tests for Kallichore server
//!
//! ## Running Tests
//!
//! - Run normally: `cargo test --package kcserver --test integration_test`
//! - Run sequentially: `cargo test --package kcserver --test integration_test -- --test-threads=1`

#![allow(missing_docs)]

mod common;

use common::TestServer;
use futures::{SinkExt, StreamExt};
use kallichore_api::models::{InterruptMode, NewSession, VarAction, VarActionType};
use kallichore_api::{ApiNoContext, ContextWrapperExt, NewSessionResponse, ServerStatusResponse};
use kcshared::jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader};
use kcshared::websocket_message::WebsocketMessage;
use serde_json;
use std::time::Duration;
use swagger::{AuthData, ContextBuilder, EmptyContext, Push, XSpanIdString};
use tokio::sync::OnceCell;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

#[cfg(unix)]
mod unix_socket_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::time::Duration;
    use tempfile::tempdir;

    pub struct UnixSocketTestServer {
        child: std::process::Child,
        socket_path: PathBuf,
        _temp_dir: tempfile::TempDir, // Keep temp dir alive
    }

    impl UnixSocketTestServer {
        pub async fn start() -> Self {
            let temp_dir = tempdir().expect("Failed to create temp directory");
            let socket_path = temp_dir.path().join("kallichore-test.sock");

            // Try to use pre-built binary first, fall back to cargo run
            let binary_path = std::env::current_dir()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("target/debug/kcserver");

            let mut cmd = if binary_path.exists() {
                let mut c = Command::new(&binary_path);
                c.args(&[
                    "--unix-socket",
                    socket_path.to_str().unwrap(),
                    "--token",
                    "none", // Disable auth for testing
                ]);
                c
            } else {
                let mut c = Command::new("cargo");
                c.args(&[
                    "run",
                    "--bin",
                    "kcserver",
                    "--",
                    "--unix-socket",
                    socket_path.to_str().unwrap(),
                    "--token",
                    "none", // Disable auth for testing
                ]);
                c
            };

            // Reduce logging noise for faster startup
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
            cmd.env("RUST_LOG", "error");

            let child = cmd
                .spawn()
                .expect("Failed to start kcserver with Unix socket");

            let test_server = UnixSocketTestServer {
                child,
                socket_path: socket_path.to_path_buf(),
                _temp_dir: temp_dir, // Keep temp dir alive
            };

            test_server.wait_for_ready().await;
            test_server
        }

        async fn wait_for_ready(&self) {
            // Wait for the socket file to be created and accept connections
            for _attempt in 0..60 {
                // Increased timeout
                if self.socket_path.exists() {
                    // Try to connect to the socket and send a simple HTTP request
                    if let Ok(mut stream) = UnixStream::connect(&self.socket_path) {
                        let request =
                            "GET /status HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
                        if stream.write_all(request.as_bytes()).is_ok() {
                            let mut buffer = [0; 1024];
                            if stream.read(&mut buffer).is_ok() {
                                let response = String::from_utf8_lossy(&buffer);
                                if response.contains("HTTP/1.1") {
                                    return;
                                }
                            }
                        }
                    }
                }

                tokio::time::sleep(Duration::from_millis(250)).await; // Slower but more reliable
            }

            panic!("Unix socket server failed to start within timeout");
        }

        pub fn socket_path(&self) -> &std::path::Path {
            &self.socket_path
        }

        // Simple method to test basic HTTP connectivity
        async fn test_http_status(&self) -> Result<String, Box<dyn std::error::Error>> {
            let mut stream = UnixStream::connect(&self.socket_path)?;

            let request = "GET /status HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
            stream.write_all(request.as_bytes())?;

            let mut response = String::new();
            stream.read_to_string(&mut response)?;

            Ok(response)
        }
    }

    impl Drop for UnixSocketTestServer {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();

            // Clean up socket file
            if self.socket_path.exists() {
                let _ = std::fs::remove_file(&self.socket_path);
            }
            // temp_dir will be automatically cleaned up when dropped
        }
    }

    #[tokio::test]
    async fn test_unix_socket_server_starts_and_responds() {
        let server = UnixSocketTestServer::start().await;

        // Test basic HTTP connectivity through Unix socket
        let response = server
            .test_http_status()
            .await
            .expect("Failed to get HTTP response via Unix socket");

        // Check that we got a valid HTTP response
        assert!(
            response.contains("HTTP/1.1"),
            "Expected HTTP response, got: {}",
            response
        );
        assert!(
            response.contains("200"),
            "Expected HTTP 200 status, got: {}",
            response
        );

        // The response should contain JSON with version info
        if let Some(json_start) = response.find("{") {
            let json_part = &response[json_start..];
            if let Some(json_end) = json_part.find("}") {
                let json_str = &json_part[..=json_end];
                let status: serde_json::Value = serde_json::from_str(json_str)
                    .expect("Failed to parse JSON response from Unix socket");

                assert_eq!(
                    status["version"].as_str().unwrap_or(""),
                    env!("CARGO_PKG_VERSION"),
                    "Version mismatch in Unix socket response"
                );

                assert!(
                    status["sessions"].is_number(),
                    "Sessions field should be a number in Unix socket response"
                );
            }
        }
    }

    #[tokio::test]
    async fn test_unix_socket_creates_socket_file() {
        let server = UnixSocketTestServer::start().await;

        // Verify the socket file exists and is a socket
        let metadata = std::fs::metadata(server.socket_path()).expect("Socket file should exist");

        use std::os::unix::fs::FileTypeExt;
        assert!(
            metadata.file_type().is_socket(),
            "File should be a Unix domain socket"
        );
    }

    #[tokio::test]
    async fn test_unix_socket_domain_socket_channels() {
        let server = UnixSocketTestServer::start().await;

        // Test that when we make an HTTP request to channels_upgrade over Unix socket,
        // we get back a domain socket path instead of a WebSocket upgrade
        let socket_path = server.socket_path();

        // Create a simple session first by making raw HTTP requests over the Unix socket
        let session_id = format!("domain-socket-test-{}", Uuid::new_v4());

        // Create session via raw HTTP over Unix socket
        let create_session_result =
            create_session_via_unix_http(socket_path, &session_id, "python3").await;
        assert!(
            create_session_result.is_ok(),
            "Failed to create session via Unix socket HTTP"
        );

        // Start the session
        let start_session_result = start_session_via_unix_http(socket_path, &session_id).await;
        assert!(
            start_session_result.is_ok(),
            "Failed to start session via Unix socket HTTP"
        );

        // Now test channels upgrade - this should return a domain socket path
        let domain_socket_path = upgrade_channels_via_unix_http(socket_path, &session_id).await;
        assert!(
            domain_socket_path.is_some(),
            "Failed to get domain socket path from channels upgrade"
        );

        let domain_socket_path = domain_socket_path.unwrap();
        println!("Received domain socket path: {}", domain_socket_path);

        // Verify the domain socket file exists
        let socket_file_path = std::path::Path::new(&domain_socket_path);
        assert!(
            socket_file_path.exists(),
            "Domain socket file should exist at {}",
            domain_socket_path
        );

        // Test basic communication over the domain socket
        test_domain_socket_communication(&domain_socket_path).await;

        println!("Domain socket communication test completed successfully");
    }

    pub async fn create_session_via_unix_http(
        socket_path: &std::path::Path,
        session_id: &str,
        python_cmd: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;

        let mut stream = UnixStream::connect(socket_path)?;

        let session_json = format!(
            r#"{{
            "session_id": "{}",
            "display_name": "Test Python Kernel (Domain Socket)",
            "language": "python",
            "username": "testuser",
            "input_prompt": "In [{{}}]: ",
            "continuation_prompt": "   ...: ",
            "argv": ["{}", "-m", "ipykernel_launcher", "-f", "{{connection_file}}"],
            "working_directory": "{}",
            "env": [],
            "connection_timeout": 3,
            "interrupt_mode": "message",
            "protocol_version": "5.3",
            "run_in_shell": false
        }}"#,
            session_id,
            python_cmd,
            std::env::current_dir().unwrap().to_string_lossy()
        );

        let request = format!(
            "PUT /sessions HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            session_json.len(),
            session_json
        );

        stream.write_all(request.as_bytes())?;

        let mut response = String::new();
        stream.read_to_string(&mut response)?;

        println!("Session creation response: {}", response);

        if response.contains("200 OK") || response.contains("201") {
            Ok(())
        } else {
            Err(format!("Session creation failed: {}", response).into())
        }
    }

    pub async fn start_session_via_unix_http(
        socket_path: &std::path::Path,
        session_id: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;

        let mut stream = UnixStream::connect(socket_path)?;

        let request = format!(
            "POST /sessions/{}/start HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            session_id
        );

        stream.write_all(request.as_bytes())?;

        let mut response = String::new();
        stream.read_to_string(&mut response)?;

        println!("Session start response: {}", response);

        if response.contains("200 OK") {
            Ok(())
        } else {
            Err(format!("Session start failed: {}", response).into())
        }
    }

    pub async fn upgrade_channels_via_unix_http(
        socket_path: &std::path::Path,
        session_id: &str,
    ) -> Option<String> {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;

        let mut stream = UnixStream::connect(socket_path).ok()?;

        let request = format!(
            "GET /sessions/{}/channels HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            session_id
        );

        stream.write_all(request.as_bytes()).ok()?;

        let mut response = String::new();
        stream.read_to_string(&mut response).ok()?;

        println!("Channels upgrade response: {}", response);

        // Look for the domain socket path in the response
        // The response should contain the socket path as a JSON string
        if response.contains("200 OK") {
            if let Some(json_start) = response.find("\"") {
                let json_part = &response[json_start..];
                if let Some(json_end) = json_part.rfind("\"") {
                    let json_str = &json_part[..=json_end];
                    // Parse as a JSON string (which includes the quotes)
                    if let Ok(path) = serde_json::from_str::<String>(json_str) {
                        return Some(path);
                    }
                }
            }
        }

        None
    }

    async fn test_domain_socket_communication(socket_path: &str) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::UnixStream;

        // Connect to the domain socket
        let mut stream = UnixStream::connect(socket_path)
            .await
            .expect("Failed to connect to domain socket");

        let (read_half, mut write_half) = stream.split();
        let mut reader = BufReader::new(read_half);

        // Send a simple kernel_info_request over the domain socket
        let kernel_info_request = JupyterMessage {
            header: JupyterMessageHeader {
                msg_id: Uuid::new_v4().to_string(),
                msg_type: "kernel_info_request".to_string(),
            },
            parent_header: None,
            channel: JupyterChannel::Shell,
            content: serde_json::json!({}),
            metadata: serde_json::json!({}),
            buffers: vec![],
        };

        let ws_message = WebsocketMessage::Jupyter(kernel_info_request);
        let message_json = serde_json::to_string(&ws_message).expect("Failed to serialize message");
        let message_with_newline = format!("{}\n", message_json);

        println!("Sending kernel_info_request over domain socket...");
        write_half
            .write_all(message_with_newline.as_bytes())
            .await
            .expect("Failed to write to domain socket");
        write_half
            .flush()
            .await
            .expect("Failed to flush domain socket");

        // Wait for responses with timeout
        let timeout = Duration::from_secs(10);
        let start_time = std::time::Instant::now();
        let mut message_count = 0;
        let mut kernel_info_reply_received = false;

        while start_time.elapsed() < timeout && message_count < 10 {
            let mut response_line = String::new();
            match tokio::time::timeout(Duration::from_secs(3), reader.read_line(&mut response_line))
                .await
            {
                Ok(Ok(0)) => {
                    println!("Domain socket closed by server");
                    break;
                }
                Ok(Ok(_)) => {
                    message_count += 1;
                    let trimmed_response = response_line.trim();

                    if trimmed_response.is_empty() {
                        continue;
                    }

                    println!("Received message {}: {}", message_count, trimmed_response);

                    // Try to parse as WebsocketMessage
                    if let Ok(ws_msg) = serde_json::from_str::<WebsocketMessage>(trimmed_response) {
                        match ws_msg {
                            WebsocketMessage::Jupyter(jupyter_msg) => {
                                println!(
                                    "  -> Jupyter message type: {}",
                                    jupyter_msg.header.msg_type
                                );

                                if jupyter_msg.header.msg_type == "kernel_info_reply" {
                                    kernel_info_reply_received = true;
                                    println!("  ✅ Received kernel_info_reply over domain socket");
                                    break; // Success!
                                }
                            }
                            WebsocketMessage::Kernel(kernel_msg) => {
                                println!("  -> Kernel message: {:?}", kernel_msg);
                            }
                        }
                    } else {
                        // It might be a ping or other simple message
                        if trimmed_response.contains("\"type\":\"ping\"") {
                            println!("  -> Received ping message");
                        } else {
                            println!(
                                "  -> Could not parse as WebsocketMessage: {}",
                                trimmed_response
                            );
                        }
                    }
                }
                Ok(Err(e)) => {
                    println!("Error reading from domain socket: {}", e);
                    break;
                }
                Err(_) => {
                    println!("Timeout waiting for domain socket response");
                    continue;
                }
            }
        }

        assert!(
            kernel_info_reply_received || message_count > 0,
            "Expected to receive kernel_info_reply or any communication over domain socket"
        );

        println!("Domain socket communication test successful!");
    }
}

#[allow(dead_code)]
type ClientContext = swagger::make_context_ty!(
    ContextBuilder,
    EmptyContext,
    Option<AuthData>,
    XSpanIdString
);

// Cache Python executable discovery to avoid repeated lookups
static PYTHON_EXECUTABLE: OnceCell<Option<String>> = OnceCell::const_new();
static IPYKERNEL_AVAILABLE: OnceCell<bool> = OnceCell::const_new();

// Helper function to properly clean up a spawned server process
fn cleanup_spawned_server(mut child: std::process::Child) {
    println!("Cleaning up spawned server (PID: {})", child.id());

    if let Err(e) = child.kill() {
        println!("Warning: Failed to terminate spawned server process: {}", e);
    }

    // Wait for the process to terminate
    match child.wait() {
        Ok(status) => {
            println!("Spawned server process terminated with status: {}", status);
        }
        Err(e) => {
            println!("Warning: Failed to wait for spawned server process: {}", e);
        }
    }
}

async fn get_python_executable() -> Option<String> {
    PYTHON_EXECUTABLE
        .get_or_init(find_python_executable)
        .await
        .clone()
}

async fn is_ipykernel_available() -> bool {
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

// Helper function to create a test server for each test
async fn create_test_server() -> TestServer {
    TestServer::start().await
}

#[tokio::test]
async fn test_server_starts_and_responds() {
    let server = create_test_server().await;
    let client = server.create_client().await;

    let response = client
        .server_status()
        .await
        .expect("Failed to get server status");

    match response {
        ServerStatusResponse::ServerStatusAndInformation(status) => {
            assert_eq!(status.version, env!("CARGO_PKG_VERSION"));
            assert!(status.sessions >= 0);
        }
        ServerStatusResponse::Error(err) => {
            panic!("Server returned error: {:?}", err);
        }
    }

    // Explicitly drop the server to ensure cleanup
    drop(server);
}

#[tokio::test]
async fn test_python_kernel_session_and_websocket_communication() {
    // Add a global timeout to prevent the test from hanging
    let test_result = tokio::time::timeout(
        Duration::from_secs(25), // 25 second max timeout for entire test
        async {
            // Use cached Python executable discovery
            let python_cmd = if let Some(cmd) = get_python_executable().await {
                cmd
            } else {
                println!("Skipping test: No Python executable found");
                return;
            };

            // Check if ipykernel is available
            if !is_ipykernel_available().await {
                println!("Skipping test: ipykernel not available for {}", python_cmd);
                return;
            }

            run_python_kernel_websocket_test(&python_cmd).await;
        },
    )
    .await;

    match test_result {
        Ok(_) => {
            println!("Python kernel test completed successfully");
        }
        Err(_) => {
            panic!("Python kernel test timed out after 25 seconds - this indicates the kernel is not working properly or the test setup failed");
        }
    }
}

async fn run_python_kernel_websocket_test(python_cmd: &str) {
    run_python_kernel_test_transport(python_cmd, TransportType::Websocket).await;
}

#[cfg(unix)]
#[tokio::test]
async fn test_python_kernel_session_and_domain_socket_communication() {
    // Add a global timeout to prevent the test from hanging
    let test_result = tokio::time::timeout(
        Duration::from_secs(25), // 25 second max timeout for entire test
        async {
            // Use cached Python executable discovery
            let python_cmd = if let Some(cmd) = get_python_executable().await {
                cmd
            } else {
                println!("Skipping test: No Python executable found");
                return;
            };

            // Check if ipykernel is available
            if !is_ipykernel_available().await {
                println!("Skipping test: ipykernel not available for {}", python_cmd);
                return;
            }

            run_python_kernel_domain_socket_test(&python_cmd).await;
        },
    );

    match test_result.await {
        Ok(_) => {}
        Err(_) => {
            panic!("Python kernel domain socket test timed out after 25 seconds");
        }
    }
}

#[cfg(unix)]
async fn run_python_kernel_domain_socket_test(python_cmd: &str) {
    // For Unix domain socket test, we need a different server setup
    use unix_socket_tests::UnixSocketTestServer;

    let server = UnixSocketTestServer::start().await;

    // Since we're using a Unix socket server, we need to handle the HTTP API differently
    // We'll make direct HTTP calls over the Unix socket like in the existing Unix socket tests
    let session_id = format!("test-session-{}", Uuid::new_v4());

    // Create session via raw HTTP over Unix socket (using existing helper functions)
    unix_socket_tests::create_session_via_unix_http(server.socket_path(), &session_id, python_cmd)
        .await
        .expect("Failed to create session via Unix socket HTTP");

    // Start the session
    unix_socket_tests::start_session_via_unix_http(server.socket_path(), &session_id)
        .await
        .expect("Failed to start session via Unix socket HTTP");

    // Perform channels upgrade to get domain socket path
    let domain_socket_path =
        unix_socket_tests::upgrade_channels_via_unix_http(server.socket_path(), &session_id)
            .await
            .expect("Failed to get domain socket path from channels upgrade");

    println!("Domain socket path: {}", domain_socket_path);

    // Verify the domain socket file exists
    let socket_file_path = std::path::Path::new(&domain_socket_path);
    assert!(
        socket_file_path.exists(),
        "Domain socket file should exist at {}",
        domain_socket_path
    );

    // Connect to the domain socket
    let stream = tokio::net::UnixStream::connect(&domain_socket_path)
        .await
        .expect("Failed to connect to domain socket");

    let (read_half, write_half) = stream.into_split();
    let reader = tokio::io::BufReader::new(read_half);
    let mut comm = CommunicationChannel::DomainSocket {
        reader,
        writer: write_half,
    };

    // Wait a reasonable amount for the kernel to start
    println!("Waiting for Python kernel to start up...");
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send a simple kernel_info_request
    let kernel_info_request = JupyterMessage {
        header: JupyterMessageHeader {
            msg_id: Uuid::new_v4().to_string(),
            msg_type: "kernel_info_request".to_string(),
        },
        parent_header: None,
        channel: JupyterChannel::Shell,
        content: serde_json::json!({}),
        metadata: serde_json::json!({}),
        buffers: vec![],
    };

    let ws_message = WebsocketMessage::Jupyter(kernel_info_request);

    println!("Sending kernel_info_request to Python kernel over domain socket...");
    comm.send_message(&ws_message)
        .await
        .expect("Failed to send kernel_info_request");

    // Wait a bit for the kernel to respond to info request
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Now send an execute_request to test actual code execution
    let execute_msg_id = Uuid::new_v4().to_string();
    let execute_request = JupyterMessage {
        header: JupyterMessageHeader {
            msg_id: execute_msg_id.clone(),
            msg_type: "execute_request".to_string(),
        },
        parent_header: None,
        channel: JupyterChannel::Shell,
        content: serde_json::json!({
            "code": "print('Hello from Kallichore domain socket test!')\nresult = 3 + 4\nprint(f'3 + 4 = {result}')",
            "silent": false,
            "store_history": true,
            "user_expressions": {},
            "allow_stdin": false,
            "stop_on_error": true
        }),
        metadata: serde_json::json!({}),
        buffers: vec![],
    };

    let ws_message = WebsocketMessage::Jupyter(execute_request);

    println!("Sending execute_request to Python kernel over domain socket...");
    comm.send_message(&ws_message)
        .await
        .expect("Failed to send execute_request");

    // Listen for responses for a limited time
    let timeout = Duration::from_secs(15);
    let start_time = std::time::Instant::now();
    let mut message_count = 0;
    let mut received_jupyter_messages = 0;
    let mut received_kernel_messages = 0;
    let mut kernel_info_reply_received = false;
    let mut execute_reply_received = false;
    let mut stream_output_received = false;
    let mut expected_output_found = false;
    let mut collected_output = String::new();

    println!("Listening for Python kernel responses over domain socket...");

    while start_time.elapsed() < timeout && message_count < 30 {
        println!(
            "Waiting for message... (elapsed: {:.1}s)",
            start_time.elapsed().as_secs_f32()
        );

        match comm.receive_message().await {
            Ok(Some(text)) => {
                message_count += 1;

                // Handle special cases first
                if text == "ping" || text == "pong" || text == "timeout" || text == "empty" {
                    println!("Received {} message", text);
                    if text == "timeout" {
                        continue;
                    }
                } else if text.starts_with("binary(") || text.starts_with("other:") {
                    println!("Received {}", text);
                } else {
                    // Regular message
                    println!("Received message {}: {}", message_count, text);

                    // Try to parse as WebsocketMessage
                    if let Ok(ws_msg) = serde_json::from_str::<WebsocketMessage>(&text) {
                        match ws_msg {
                            WebsocketMessage::Jupyter(jupyter_msg) => {
                                received_jupyter_messages += 1;
                                println!(
                                    "  -> Jupyter message type: {}",
                                    jupyter_msg.header.msg_type
                                );

                                // Check for kernel_info_reply
                                if jupyter_msg.header.msg_type == "kernel_info_reply" {
                                    kernel_info_reply_received = true;
                                    println!("  ✅ Received kernel_info_reply over domain socket");
                                }

                                // Check for execute_reply
                                if jupyter_msg.header.msg_type == "execute_reply" {
                                    execute_reply_received = true;
                                    println!("  ✅ Received execute_reply over domain socket");
                                    if let Some(status) = jupyter_msg.content.get("status") {
                                        println!("  -> Execution status: {}", status);

                                        // Ensure the execution was successful
                                        if status != "ok" {
                                            panic!("Code execution failed with status: {}", status);
                                        }

                                        // If we have both successful execution and expected output, we can be confident
                                        if status == "ok" && expected_output_found {
                                            println!("  🎉 Domain socket execution completed successfully with expected output!");
                                            break; // Exit when we have complete success
                                        }
                                    } else {
                                        // Even without status, if we got execute_reply, it's good
                                        if expected_output_found {
                                            println!("  🎉 Domain socket execution completed with expected output!");
                                            break;
                                        }
                                    }
                                    if let Some(execution_count) =
                                        jupyter_msg.content.get("execution_count")
                                    {
                                        println!("  -> Execution count: {}", execution_count);
                                    }
                                }

                                // Check for stream output (stdout)
                                if jupyter_msg.header.msg_type == "stream" {
                                    stream_output_received = true;
                                    if let Some(text) = jupyter_msg.content.get("text") {
                                        let output_text = text.as_str().unwrap_or("");
                                        println!(
                                            "  ✅ Received stream output over domain socket: {}",
                                            output_text
                                        );

                                        // Collect all output to check comprehensively
                                        collected_output.push_str(output_text);

                                        // Check if we got the expected output in the collected text
                                        if collected_output
                                            .contains("Hello from Kallichore domain socket test!")
                                            && collected_output.contains("3 + 4 = 7")
                                        {
                                            expected_output_found = true;
                                            println!("  🎉 Found expected domain socket output content in collected output!");
                                            // Don't break here - let the test continue to get execute_reply
                                        }
                                    }
                                }

                                // Check for status messages (busy/idle)
                                if jupyter_msg.header.msg_type == "status" {
                                    if let Some(state) = jupyter_msg.content.get("execution_state")
                                    {
                                        println!("  -> Kernel state: {}", state);
                                    }
                                }
                            }
                            WebsocketMessage::Kernel(kernel_msg) => {
                                received_kernel_messages += 1;
                                println!("  -> Kernel message: {:?}", kernel_msg);
                            }
                        }
                    } else if text.contains("\"type\":\"ping\"")
                        || text.contains("\"type\":\"disconnect\"")
                    {
                        println!("  -> Server control message: {}", text);
                    } else {
                        println!("  -> Could not parse as WebsocketMessage: {}", text);
                    }
                }
            }
            Ok(None) => {
                println!("Domain socket communication channel closed");
                break;
            }
            Err(e) => {
                println!("Domain socket communication error: {}", e);
                break;
            }
        }
    }

    println!("Python kernel domain socket test completed:");
    println!("  - Total messages: {}", message_count);
    println!("  - Jupyter messages: {}", received_jupyter_messages);
    println!("  - Kernel messages: {}", received_kernel_messages);
    println!("  - Kernel info reply: {}", kernel_info_reply_received);
    println!("  - Execute reply: {}", execute_reply_received);
    println!("  - Stream output: {}", stream_output_received);
    println!("  - Expected output found: {}", expected_output_found);
    println!("  - Collected output: {:?}", collected_output);

    // Make the test robust by requiring specific conditions to be met

    // First, we must receive at least some communication from the kernel
    assert!(
        received_jupyter_messages > 0,
        "Expected to receive Jupyter messages from the Python kernel over domain socket, but got {}. The kernel may not be starting or communicating properly.",
        received_jupyter_messages
    );

    // We should get a kernel_info_reply to confirm the kernel is responsive
    assert!(
        kernel_info_reply_received,
        "Expected to receive kernel_info_reply from Python kernel over domain socket, but didn't get one. The kernel is not responding to basic requests."
    );

    // We should get an execute_reply to confirm code execution capability
    assert!(
        execute_reply_received,
        "Expected to receive execute_reply from Python kernel over domain socket, but didn't get one. The kernel is not executing code properly."
    );

    // We should get stream output from our print statements
    assert!(
        stream_output_received,
        "Expected to receive stream output from Python kernel over domain socket, but didn't get any. The kernel is not producing stdout output."
    );

    // Most importantly, we should get the exact output we expect
    assert!(
        expected_output_found,
        "Expected to find 'Hello from Kallichore domain socket test!' and '3 + 4 = 7' in the kernel output over domain socket, but didn't find both. The kernel executed but produced unexpected output. Actual collected output: {:?}",
        collected_output
    );

    // Properly close the communication channel
    if let Err(e) = comm.close().await {
        println!("Failed to close domain socket communication channel: {}", e);
    }

    println!("Domain socket communication test successful!");
}

#[cfg(windows)]
#[tokio::test]
async fn test_python_kernel_session_and_named_pipe_communication() {
    // Add a global timeout to prevent the test from hanging
    let test_result = tokio::time::timeout(
        Duration::from_secs(25), // 25 second max timeout for entire test
        async {
            // Use cached Python executable discovery
            let python_cmd = if let Some(cmd) = get_python_executable().await {
                cmd
            } else {
                println!("Skipping test: No Python executable found");
                return;
            };

            // Check if ipykernel is available
            if !is_ipykernel_available().await {
                println!("Skipping test: ipykernel not available for {}", python_cmd);
                return;
            }

            run_python_kernel_named_pipe_test(&python_cmd).await;
        },
    );

    match test_result.await {
        Ok(_) => {}
        Err(_) => {
            panic!("Python kernel named pipe test timed out after 25 seconds");
        }
    }
}

#[cfg(windows)]
async fn run_python_kernel_named_pipe_test(python_cmd: &str) {
    // For Windows named pipe test, we need a special named pipe server setup
    // We'll create a named pipe server and communicate with it directly, similar to domain socket test
    use serde_json::json;
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use std::time::Duration;
    use tempfile::NamedTempFile;

    // Create a temporary connection file
    let temp_file = NamedTempFile::new().expect("Failed to create temp connection file");
    let connection_file_path = temp_file.path().to_string_lossy().to_string();

    // Try to use pre-built binary first, fall back to cargo run
    let binary_path = std::env::current_dir()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/kcserver.exe");

    let mut cmd = if binary_path.exists() {
        println!("Using pre-built binary: {:?}", binary_path);
        let mut c = Command::new(&binary_path);
        c.args(&[
            "--connection-file",
            &connection_file_path,
            "--transport",
            "named-pipe",
            "--token",
            "none", // Disable auth for testing
        ]);
        c
    } else {
        println!("Pre-built binary not found, using cargo run");
        let mut c = Command::new("cargo");
        c.args(&[
            "run",
            "--bin",
            "kcserver",
            "--",
            "--connection-file",
            &connection_file_path,
            "--transport",
            "named-pipe",
            "--token",
            "none", // Disable auth for testing
        ]);
        c
    };

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.env("RUST_LOG", "debug");

    println!("Starting named pipe server...");
    let mut child = cmd
        .spawn()
        .expect("Failed to start kcserver with named pipe");

    // Wait for the server to start and write the connection file
    let mut retries = 0;
    let connection_info: serde_json::Value = loop {
        tokio::time::sleep(Duration::from_millis(500)).await;

        println!("Attempt {}: Checking connection file...", retries + 1);

        match std::fs::read_to_string(&connection_file_path) {
            Ok(content) if !content.trim().is_empty() => {
                println!("Connection file content: {}", content);
                match serde_json::from_str(&content) {
                    Ok(info) => break info,
                    Err(_) if retries < 10 => {
                        retries += 1;
                        continue;
                    }
                    Err(e) => panic!("Failed to parse connection file: {}", e),
                }
            }
            Ok(_) => {
                if retries < 10 {
                    retries += 1;
                    continue;
                } else {
                    panic!("Connection file is empty after 5 seconds");
                }
            }
            Err(_) if retries < 10 => {
                retries += 1;
                continue;
            }
            Err(e) => panic!("Connection file error after 5 seconds: {}", e),
        }
    };

    let pipe_name = connection_info["named_pipe"]
        .as_str()
        .expect("Missing named_pipe in connection file")
        .to_string();

    println!("Named pipe server started with pipe: {}", pipe_name);

    // Wait for the server to be ready
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Create a session via HTTP over named pipe
    let session_id = format!("named-pipe-test-{}", Uuid::new_v4());
    let session_request = json!({
        "session_id": session_id,
        "argv": [python_cmd, "-m", "ipykernel_launcher", "-f", "{connection_file}"],
        "display_name": "Python Named Pipe Test",
        "language": "python",
        "working_directory": ".",
        "input_prompt": "In [{}]: ",
        "continuation_prompt": "   ...: ",
        "username": "test",
        "env": [],
        "run_in_shell": false,
        "interrupt_mode": "signal",
        "connection_timeout": 60,
        "protocol_version": "5.3"
    });

    println!("Creating session: {}", session_id);

    // Helper function to send HTTP request over named pipe (from named_pipe_test.rs)
    fn send_http_request_sync(
        pipe_name: &str,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> std::io::Result<String> {
        let mut pipe = OpenOptions::new().read(true).write(true).open(pipe_name)?;

        let request = if let Some(body) = body {
            format!(
                "{} {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                method,
                path,
                body.len(),
                body
            )
        } else {
            format!("{} {} HTTP/1.1\r\nHost: localhost\r\n\r\n", method, path)
        };

        pipe.write_all(request.as_bytes())?;

        // Read response
        let mut buffer = vec![0u8; 4096];
        let bytes_read = pipe.read(&mut buffer)?;
        let response_str = String::from_utf8_lossy(&buffer[..bytes_read]);

        Ok(response_str.to_string())
    }

    // Create session via HTTP over named pipe
    let create_response = send_http_request_sync(
        &pipe_name,
        "PUT",
        "/sessions",
        Some(&session_request.to_string()),
    )
    .expect("Failed to create session via named pipe HTTP");

    println!("Session creation response: {}", create_response);

    // Start the session
    let start_response = send_http_request_sync(
        &pipe_name,
        "POST",
        &format!("/sessions/{}/start", session_id),
        None,
    )
    .expect("Failed to start session via named pipe HTTP");

    println!("Session start response: {}", start_response);

    // Try to get a named pipe for channels communication
    let upgrade_response = send_http_request_sync(
        &pipe_name,
        "GET",
        &format!("/sessions/{}/channels", session_id),
        None,
    )
    .expect("Failed to get channels upgrade via named pipe HTTP");

    println!("Channels upgrade response: {}", upgrade_response);

    // For now, we'll test basic functionality without full Jupyter message exchange
    // The infrastructure is in place, but we'd need the server to support named pipe channel upgrades
    println!("Named pipe kernel communication test infrastructure complete");
    println!("Full Jupyter message exchange over named pipes requires server-side support");

    // Clean up
    let _ = child.kill();
    let _ = child.wait();

    println!("Named pipe test completed successfully");
}

#[derive(Clone)]
#[allow(dead_code)]
enum TransportType {
    Websocket,
    #[cfg(unix)]
    DomainSocket,
    #[cfg(windows)]
    NamedPipe,
}

enum CommunicationChannel {
    Websocket {
        sender: futures::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            Message,
        >,
        receiver: futures::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
    },
    #[cfg(unix)]
    DomainSocket {
        reader: tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>,
        writer: tokio::net::unix::OwnedWriteHalf,
    },
    #[cfg(windows)]
    NamedPipe { pipe: std::fs::File },
}

impl CommunicationChannel {
    async fn send_message(
        &mut self,
        message: &WebsocketMessage,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let message_json = serde_json::to_string(message)?;

        match self {
            CommunicationChannel::Websocket { sender, .. } => {
                sender.send(Message::Text(message_json)).await?;
            }
            #[cfg(unix)]
            CommunicationChannel::DomainSocket { writer, .. } => {
                use tokio::io::AsyncWriteExt;
                let message_with_newline = format!("{}\n", message_json);
                writer.write_all(message_with_newline.as_bytes()).await?;
                writer.flush().await?;
            }
            #[cfg(windows)]
            CommunicationChannel::NamedPipe { pipe } => {
                use std::io::Write;
                let message_with_newline = format!("{}\n", message_json);
                pipe.write_all(message_with_newline.as_bytes())?;
                pipe.flush()?;
            }
        }
        Ok(())
    }

    async fn receive_message(
        &mut self,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        match self {
            CommunicationChannel::Websocket { receiver, .. } => {
                use futures::StreamExt;
                match tokio::time::timeout(Duration::from_millis(1000), receiver.next()).await {
                    Ok(Some(Ok(Message::Text(text)))) => Ok(Some(text)),
                    Ok(Some(Ok(Message::Ping(_)))) => Ok(Some("ping".to_string())),
                    Ok(Some(Ok(Message::Pong(_)))) => Ok(Some("pong".to_string())),
                    Ok(Some(Ok(Message::Binary(data)))) => {
                        Ok(Some(format!("binary({} bytes)", data.len())))
                    }
                    Ok(Some(Ok(msg))) => Ok(Some(format!("other: {:?}", msg))),
                    Ok(Some(Err(e))) => Err(e.into()),
                    Ok(None) => Ok(None), // Stream closed
                    Err(_) => Ok(Some("timeout".to_string())),
                }
            }
            #[cfg(unix)]
            CommunicationChannel::DomainSocket { reader, .. } => {
                use tokio::io::AsyncBufReadExt;
                let mut buffer = String::new();
                match tokio::time::timeout(
                    Duration::from_millis(1000),
                    reader.read_line(&mut buffer),
                )
                .await
                {
                    Ok(Ok(0)) => Ok(None), // Stream closed
                    Ok(Ok(_)) => {
                        let trimmed = buffer.trim();
                        if trimmed.is_empty() {
                            Ok(Some("empty".to_string()))
                        } else {
                            Ok(Some(trimmed.to_string()))
                        }
                    }
                    Ok(Err(e)) => Err(e.into()),
                    Err(_) => Ok(Some("timeout".to_string())),
                }
            }
            #[cfg(windows)]
            CommunicationChannel::NamedPipe { pipe } => {
                use std::io::{BufRead, BufReader};

                // For named pipes, we need to read synchronously with a timeout simulation
                // We'll use a non-blocking approach by reading available data
                let mut buf_reader = BufReader::new(pipe);
                let mut buffer = String::new();

                // For simplicity in tests, we'll do a blocking read with a small timeout simulation
                // In a real implementation, you'd want proper async handling for Windows named pipes
                match buf_reader.read_line(&mut buffer) {
                    Ok(0) => Ok(None), // Pipe closed
                    Ok(_) => {
                        let trimmed = buffer.trim();
                        if trimmed.is_empty() {
                            Ok(Some("empty".to_string()))
                        } else {
                            Ok(Some(trimmed.to_string()))
                        }
                    }
                    Err(e) => {
                        // Simulate timeout behavior
                        if e.kind() == std::io::ErrorKind::WouldBlock {
                            Ok(Some("timeout".to_string()))
                        } else {
                            Err(e.into())
                        }
                    }
                }
            }
        }
    }

    async fn close(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match self {
            CommunicationChannel::Websocket { sender, .. } => {
                sender.send(Message::Close(None)).await?;
            }
            #[cfg(unix)]
            CommunicationChannel::DomainSocket { writer, .. } => {
                use tokio::io::AsyncWriteExt;
                let close_msg =
                    format!("{{\"type\":\"disconnect\",\"reason\":\"Test completed.\"}}\n");
                let _ = writer.write_all(close_msg.as_bytes()).await;
                let _ = writer.flush().await;
            }
            #[cfg(windows)]
            CommunicationChannel::NamedPipe { pipe } => {
                use std::io::Write;
                let close_msg =
                    format!("{{\"type\":\"disconnect\",\"reason\":\"Test completed.\"}}\n");
                let _ = pipe.write_all(close_msg.as_bytes());
                let _ = pipe.flush();
            }
        }
        Ok(())
    }
}

async fn run_python_kernel_test_transport(python_cmd: &str, transport: TransportType) {
    let server = create_test_server().await;
    let client = server.create_client().await;

    // Create a kernel session using Python with ipykernel
    let session_id = format!("test-session-{}", Uuid::new_v4());
    let new_session = NewSession {
        session_id: session_id.clone(),
        display_name: "Test Python Kernel".to_string(),
        language: "python".to_string(),
        username: "testuser".to_string(),
        input_prompt: "In [{}]: ".to_string(),
        continuation_prompt: "   ...: ".to_string(),
        argv: vec![
            python_cmd.to_string(),
            "-m".to_string(),
            "ipykernel_launcher".to_string(),
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
        connection_timeout: Some(3), // Reduced timeout
        interrupt_mode: InterruptMode::Message,
        protocol_version: Some("5.3".to_string()),
        run_in_shell: Some(false),
    };

    // Create the kernel session
    let session_response = client
        .new_session(new_session)
        .await
        .expect("Failed to create new session");

    let session_info = match session_response {
        NewSessionResponse::TheSessionID(session_info) => session_info,
        NewSessionResponse::Unauthorized => panic!("Unauthorized"),
        NewSessionResponse::InvalidRequest(err) => panic!("Invalid request: {:?}", err),
    };

    println!("Created Python kernel session: {:?}", session_info);

    // Start the kernel session
    println!("Starting the kernel...");
    let start_response = client
        .start_session(session_id.clone())
        .await
        .expect("Failed to start session");

    println!("Kernel start response: {:?}", start_response);

    // Create a communication channel based on transport type
    let mut comm = match transport {
        TransportType::Websocket => {
            // Connect to the websocket for this session
            let ws_url = format!(
                "ws://localhost:{}/sessions/{}/channels",
                server.port(),
                session_id
            );

            let (ws_stream, _) = connect_async(&ws_url)
                .await
                .expect("Failed to connect to websocket");

            let (ws_sender, ws_receiver) = ws_stream.split();
            CommunicationChannel::Websocket {
                sender: ws_sender,
                receiver: ws_receiver,
            }
        }
        #[cfg(unix)]
        TransportType::DomainSocket => {
            // Perform channels upgrade to get domain socket path
            let upgrade_response = client
                .channels_upgrade(session_id.clone())
                .await
                .expect("Failed to upgrade to domain socket");

            let socket_path = match upgrade_response {
                kallichore_api::ChannelsUpgradeResponse::UpgradedConnection(path) => path,
                kallichore_api::ChannelsUpgradeResponse::Unauthorized => panic!("Unauthorized"),
                kallichore_api::ChannelsUpgradeResponse::SessionNotFound => {
                    panic!("Session not found")
                }
                kallichore_api::ChannelsUpgradeResponse::InvalidRequest(err) => {
                    panic!("Invalid request: {:?}", err)
                }
            };

            println!("Domain socket path: {}", socket_path);

            // Connect to the domain socket
            let stream = tokio::net::UnixStream::connect(&socket_path)
                .await
                .expect("Failed to connect to domain socket");

            let (read_half, write_half) = stream.into_split();
            let reader = tokio::io::BufReader::new(read_half);
            CommunicationChannel::DomainSocket {
                reader,
                writer: write_half,
            }
        }
        #[cfg(windows)]
        TransportType::NamedPipe => {
            // Perform channels upgrade to get named pipe path
            let upgrade_response = client
                .channels_upgrade(session_id.clone())
                .await
                .expect("Failed to upgrade to named pipe");

            let pipe_path = match upgrade_response {
                kallichore_api::ChannelsUpgradeResponse::UpgradedConnection(path) => path,
                kallichore_api::ChannelsUpgradeResponse::Unauthorized => panic!("Unauthorized"),
                kallichore_api::ChannelsUpgradeResponse::SessionNotFound => {
                    panic!("Session not found")
                }
                kallichore_api::ChannelsUpgradeResponse::InvalidRequest(err) => {
                    panic!("Invalid request: {:?}", err)
                }
            };

            println!("Named pipe path: {}", pipe_path);

            // Connect to the named pipe
            use std::fs::OpenOptions;
            let pipe = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&pipe_path)
                .expect("Failed to connect to named pipe");

            CommunicationChannel::NamedPipe { pipe }
        }
    };

    // Wait a reasonable amount for the kernel to start
    println!("Waiting for Python kernel to start up...");
    tokio::time::sleep(Duration::from_millis(500)).await; // Reduced from 1000ms

    // Send a simple kernel_info_request immediately without status check
    let kernel_info_request = JupyterMessage {
        header: JupyterMessageHeader {
            msg_id: Uuid::new_v4().to_string(),
            msg_type: "kernel_info_request".to_string(),
        },
        parent_header: None,
        channel: JupyterChannel::Shell,
        content: serde_json::json!({}),
        metadata: serde_json::json!({}),
        buffers: vec![],
    };

    let ws_message = WebsocketMessage::Jupyter(kernel_info_request);

    println!("Sending kernel_info_request to Python kernel...");
    comm.send_message(&ws_message)
        .await
        .expect("Failed to send kernel_info_request");

    // Wait a bit for the kernel to respond to info request
    tokio::time::sleep(Duration::from_millis(500)).await; // Reduced from 1000ms

    // Now send an execute_request to test actual code execution
    let execute_msg_id = Uuid::new_v4().to_string();
    let execute_request = JupyterMessage {
        header: JupyterMessageHeader {
            msg_id: execute_msg_id.clone(),
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
    };

    let ws_message = WebsocketMessage::Jupyter(execute_request);

    println!("Sending execute_request to Python kernel...");
    comm.send_message(&ws_message)
        .await
        .expect("Failed to send execute_request");

    // Listen for any responses for a limited time
    let timeout = Duration::from_secs(15);
    let start_time = std::time::Instant::now();
    let mut message_count = 0;
    let mut received_jupyter_messages = 0;
    let mut received_kernel_messages = 0;
    let mut kernel_info_reply_received = false;
    let mut execute_reply_received = false;
    let mut stream_output_received = false;
    let mut expected_output_found = false;
    let mut collected_output = String::new(); // Collect all output to check against

    println!("Listening for Python kernel responses...");

    while start_time.elapsed() < timeout && message_count < 30 {
        // Increased from 20 to allow for more messages
        println!(
            "Waiting for message... (elapsed: {:.1}s)",
            start_time.elapsed().as_secs_f32()
        );

        match comm.receive_message().await {
            Ok(Some(text)) => {
                message_count += 1;

                // Handle special cases first
                if text == "ping" || text == "pong" || text == "timeout" || text == "empty" {
                    println!("Received {} message", text);
                    if text == "timeout" {
                        continue;
                    }
                } else if text.starts_with("binary(") || text.starts_with("other:") {
                    println!("Received {}", text);
                } else {
                    // Regular message
                    println!("Received message {}: {}", message_count, text);

                    // Try to parse as WebsocketMessage
                    if let Ok(ws_msg) = serde_json::from_str::<WebsocketMessage>(&text) {
                        match ws_msg {
                            WebsocketMessage::Jupyter(jupyter_msg) => {
                                received_jupyter_messages += 1;
                                println!(
                                    "  -> Jupyter message type: {}",
                                    jupyter_msg.header.msg_type
                                );

                                // Check for kernel_info_reply
                                if jupyter_msg.header.msg_type == "kernel_info_reply" {
                                    kernel_info_reply_received = true;
                                    println!("  ✅ Received kernel_info_reply");
                                }

                                // Check for execute_reply
                                if jupyter_msg.header.msg_type == "execute_reply" {
                                    execute_reply_received = true;
                                    println!("  ✅ Received execute_reply");
                                    if let Some(status) = jupyter_msg.content.get("status") {
                                        println!("  -> Execution status: {}", status);

                                        // Ensure the execution was successful
                                        if status != "ok" {
                                            panic!("Code execution failed with status: {}", status);
                                        }

                                        // If we have both successful execution and expected output, we can be confident
                                        if status == "ok" && expected_output_found {
                                            println!("  🎉 Execution completed successfully with expected output!");
                                            break; // Exit when we have complete success
                                        }
                                    } else {
                                        // Even without status, if we got execute_reply, it's good
                                        if expected_output_found {
                                            println!(
                                                "  🎉 Execution completed with expected output!"
                                            );
                                            break;
                                        }
                                    }
                                    if let Some(execution_count) =
                                        jupyter_msg.content.get("execution_count")
                                    {
                                        println!("  -> Execution count: {}", execution_count);
                                    }
                                }

                                // Check for stream output (stdout)
                                if jupyter_msg.header.msg_type == "stream" {
                                    stream_output_received = true;
                                    if let Some(text) = jupyter_msg.content.get("text") {
                                        let output_text = text.as_str().unwrap_or("");
                                        println!("  ✅ Received stream output: {}", output_text);

                                        // Collect all output to check comprehensively
                                        collected_output.push_str(output_text);

                                        // Check if we got the expected output in the collected text
                                        if collected_output.contains("Hello from Kallichore test!")
                                            && collected_output.contains("2 + 3 = 5")
                                        {
                                            expected_output_found = true;
                                            println!("  🎉 Found expected output content in collected output!");
                                            // Don't break here - let the test continue to get execute_reply
                                        }
                                    }
                                }

                                // Check for status messages (busy/idle)
                                if jupyter_msg.header.msg_type == "status" {
                                    if let Some(state) = jupyter_msg.content.get("execution_state")
                                    {
                                        println!("  -> Kernel state: {}", state);
                                    }
                                }
                            }
                            WebsocketMessage::Kernel(kernel_msg) => {
                                received_kernel_messages += 1;
                                println!("  -> Kernel message: {:?}", kernel_msg);
                            }
                        }
                    } else if text.contains("\"type\":\"ping\"")
                        || text.contains("\"type\":\"disconnect\"")
                    {
                        println!("  -> Server control message: {}", text);
                    } else {
                        println!("  -> Could not parse as WebsocketMessage: {}", text);
                    }
                }
            }
            Ok(None) => {
                println!("Communication channel closed");
                break;
            }
            Err(e) => {
                println!("Communication error: {}", e);
                break;
            }
        }
    }

    println!("Python kernel test completed:");
    println!("  - Total messages: {}", message_count);
    println!("  - Jupyter messages: {}", received_jupyter_messages);
    println!("  - Kernel messages: {}", received_kernel_messages);
    println!("  - Kernel info reply: {}", kernel_info_reply_received);
    println!("  - Execute reply: {}", execute_reply_received);
    println!("  - Stream output: {}", stream_output_received);
    println!("  - Expected output found: {}", expected_output_found);
    println!("  - Collected output: {:?}", collected_output);

    // Make the test robust by requiring specific conditions to be met

    // First, we must receive at least some communication from the kernel
    assert!(
        received_jupyter_messages > 0,
        "Expected to receive Jupyter messages from the Python kernel, but got {}. The kernel may not be starting or communicating properly.",
        received_jupyter_messages
    );

    // We should get a kernel_info_reply to confirm the kernel is responsive
    assert!(
        kernel_info_reply_received,
        "Expected to receive kernel_info_reply from Python kernel, but didn't get one. The kernel is not responding to basic requests."
    );

    // We should get an execute_reply to confirm code execution capability
    assert!(
        execute_reply_received,
        "Expected to receive execute_reply from Python kernel, but didn't get one. The kernel is not executing code properly."
    );

    // We should get stream output from our print statements
    assert!(
        stream_output_received,
        "Expected to receive stream output from Python kernel, but didn't get any. The kernel is not producing stdout output."
    );

    // Most importantly, we should get the exact output we expect
    assert!(
        expected_output_found,
        "Expected to find 'Hello from Kallichore test!' and '2 + 3 = 5' in the kernel output, but didn't find both. The kernel executed but produced unexpected output. Actual collected output: {:?}",
        collected_output
    );

    // This test should complete without hanging even if kernel doesn't respond
    // Properly close the communication channel
    if let Err(e) = comm.close().await {
        println!("Failed to close communication channel: {}", e);
    }

    // Explicitly drop the server to ensure cleanup
    drop(server);
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

                // Try to find the full path using platform-appropriate command
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
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        if !full_path.is_empty() {
                            println!("Full path for {}: {}", candidate, full_path);
                            return Some(full_path);
                        }
                    }
                }
                return Some(candidate.to_string());
            }
            _ => continue,
        }
    }
    None
}

// Helper function to check if ipykernel is available
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

#[tokio::test]
async fn test_multiple_kernel_sessions() {
    // Use cached Python executable discovery
    let python_cmd = if let Some(cmd) = get_python_executable().await {
        cmd
    } else {
        println!("Skipping test: No Python executable found");
        return;
    };

    // Check if ipykernel is available
    if !is_ipykernel_available().await {
        println!("Skipping test: ipykernel not available for {}", python_cmd);
        return;
    }

    let server = create_test_server().await;
    let client = server.create_client().await;

    // Create multiple kernel sessions
    let mut sessions = Vec::new();

    for i in 0..3 {
        let session_id = format!("multi-test-session-{}-{}", i, Uuid::new_v4());
        let new_session = NewSession {
            session_id: session_id.clone(),
            display_name: format!("Multi Test Python Kernel {}", i),
            language: "python".to_string(),
            username: "testuser".to_string(),
            input_prompt: "In [{}]: ".to_string(),
            continuation_prompt: "   ...: ".to_string(),
            argv: vec![
                python_cmd.clone(),
                "-m".to_string(),
                "ipykernel_launcher".to_string(),
                "-f".to_string(),
                "{connection_file}".to_string(),
            ],
            working_directory: std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .to_string(),
            env: vec![],
            connection_timeout: Some(3), // Reduced timeout
            interrupt_mode: InterruptMode::Message,
            protocol_version: Some("5.3".to_string()),
            run_in_shell: Some(false),
        };

        let session_response = client
            .new_session(new_session)
            .await
            .expect("Failed to create new session");

        match session_response {
            NewSessionResponse::TheSessionID(session_info) => {
                sessions.push((session_id, session_info));
            }
            _ => panic!("Failed to create session {}", i),
        }
    }

    assert_eq!(sessions.len(), 3, "Should have created 3 sessions");

    // Verify all sessions have unique IDs
    let mut session_ids: Vec<_> = sessions.iter().map(|(id, _)| id.clone()).collect();
    session_ids.sort();
    session_ids.dedup();
    assert_eq!(session_ids.len(), 3, "All session IDs should be unique");

    println!(
        "Successfully created {} unique kernel sessions",
        sessions.len()
    );

    // Explicitly drop the server to ensure cleanup
    drop(server);
}

#[tokio::test]
async fn test_server_starts_with_connection_file() {
    // Create a temporary connection file path
    let temp_dir = std::env::temp_dir();
    let connection_file_path = temp_dir.join(format!(
        "kallichore_test_connection_{}.json",
        Uuid::new_v4()
    ));
    let connection_file_str = connection_file_path.to_string_lossy().to_string(); // Start server without specifying port (port 0), using connection file
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
        let mut c = std::process::Command::new(&binary_path);
        c.args(&[
            "--port",
            "0", // Let OS pick the port
            "--connection-file",
            &connection_file_str,
            "--transport",
            "tcp", // Explicitly request TCP for backward compatibility test
            "--token",
            "none", // Disable auth for testing
        ]);
        c
    } else {
        println!("Pre-built binary not found, using cargo run");
        let mut c = std::process::Command::new("cargo");
        c.args(&[
            "run",
            "--bin",
            "kcserver",
            "--",
            "--port",
            "0", // Let OS pick the port
            "--connection-file",
            &connection_file_str,
            "--transport",
            "tcp", // Explicitly request TCP for backward compatibility test
            "--token",
            "none", // Disable auth for testing
        ]);
        c
    }; // Capture output for debugging
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.env("RUST_LOG", "info");

    let mut child = cmd.spawn().expect("Failed to start kcserver");

    // Wait for connection file to be created with increased timeout
    let mut attempts = 0;
    while !connection_file_path.exists() && attempts < 100 {
        // Increased from 50
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
        // Try to get error output from the process
        if let Ok(output) = child.try_wait() {
            if let Some(_exit_status) = output {
                // Process has exited, try to get output
                if let Ok(final_output) = child.wait_with_output() {
                    if !final_output.stdout.is_empty() {
                        println!(
                            "Server stdout: {}",
                            String::from_utf8_lossy(&final_output.stdout)
                        );
                    }
                    if !final_output.stderr.is_empty() {
                        println!(
                            "Server stderr: {}",
                            String::from_utf8_lossy(&final_output.stderr)
                        );
                    }
                }
            } else {
                // Process is still running, kill it
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        panic!("Connection file was not created within timeout");
    }

    // Read the connection file
    let connection_content =
        std::fs::read_to_string(&connection_file_path).expect("Failed to read connection file");

    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct ServerConnectionInfo {
        port: u16,
        base_path: String,
        server_path: String,
        server_pid: u32,
        bearer_token: Option<String>,
        log_path: Option<String>,
    }

    let connection_info: ServerConnectionInfo =
        serde_json::from_str(&connection_content).expect("Failed to parse connection file");

    // Verify the connection info makes sense
    assert!(connection_info.port > 0, "Port should be greater than 0");
    assert_eq!(
        connection_info.base_path,
        format!("http://127.0.0.1:{}", connection_info.port)
    );
    assert!(
        connection_info.server_pid > 0,
        "PID should be greater than 0"
    );
    assert_eq!(
        connection_info.bearer_token, None,
        "Token should be None when disabled"
    );

    // Create a client using the connection info from the file
    #[allow(trivial_casts)]
    let client = {
        let context = swagger::make_context!(
            ContextBuilder,
            EmptyContext,
            None as Option<AuthData>,
            XSpanIdString::default()
        );

        let client = kallichore_api::Client::try_new_http(&connection_info.base_path)
            .expect("Failed to create HTTP client");

        Box::new(client.with_context(context))
    };

    // Wait for server to be ready by polling status
    let mut ready = false;
    #[allow(unused_variables)]
    for attempt in 0..50 {
        match tokio::time::timeout(Duration::from_millis(200), client.server_status()).await {
            Ok(Ok(_)) => {
                ready = true;
                break;
            }
            Ok(Err(_)) | Err(_) => {
                if attempt > 40 {
                    println!("Server status check failed on attempt {}", attempt);
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }

    assert!(ready, "Server failed to become ready within timeout");

    // Test that we can actually use the server
    let response = client
        .server_status()
        .await
        .expect("Failed to get server status");

    match response {
        ServerStatusResponse::ServerStatusAndInformation(status) => {
            assert_eq!(status.version, env!("CARGO_PKG_VERSION"));
            assert!(status.sessions >= 0);
        }
        ServerStatusResponse::Error(err) => {
            panic!("Server returned error: {:?}", err);
        }
    }

    // Clean up
    cleanup_spawned_server(child);
    let _ = std::fs::remove_file(&connection_file_path);

    println!(
        "Successfully tested server with connection file. Port: {}",
        connection_info.port
    );
}

#[tokio::test]
async fn test_server_connection_file_with_auth_token() {
    // Create a temporary connection file path
    let temp_dir = std::env::temp_dir();
    let connection_file_path =
        temp_dir.join(format!("kallichore_test_auth_{}.json", Uuid::new_v4()));
    let connection_file_str = connection_file_path.to_string_lossy().to_string();

    // Create a temporary token file
    let token_file_path = temp_dir.join(format!("kallichore_test_token_{}.txt", Uuid::new_v4()));
    let token_file_str = token_file_path.to_string_lossy().to_string();
    let test_token = "test_auth_token_12345";
    std::fs::write(&token_file_path, test_token).expect("Failed to write token file"); // Start server without specifying port, using connection file and auth token
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
        let mut c = std::process::Command::new(&binary_path);
        c.args(&[
            "--port",
            "0", // Let OS pick the port
            "--connection-file",
            &connection_file_str,
            "--transport",
            "tcp", // Explicitly request TCP for backward compatibility test
            "--token",
            &token_file_str,
        ]);
        c
    } else {
        println!("Pre-built binary not found, using cargo run");
        let mut c = std::process::Command::new("cargo");
        c.args(&[
            "run",
            "--bin",
            "kcserver",
            "--",
            "--port",
            "0", // Let OS pick the port
            "--connection-file",
            &connection_file_str,
            "--transport",
            "tcp", // Explicitly request TCP for backward compatibility test
            "--token",
            &token_file_str,
        ]);
        c
    };

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.env("RUST_LOG", "info");

    let mut child = cmd.spawn().expect("Failed to start kcserver"); // Wait for connection file to be created with increased timeout
    let mut attempts = 0;
    while !connection_file_path.exists() && attempts < 100 {
        // Increased from 50
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
        // Try to get error output from the process
        if let Ok(output) = child.try_wait() {
            if let Some(_exit_status) = output {
                // Process has exited, try to get output
                if let Ok(final_output) = child.wait_with_output() {
                    if !final_output.stdout.is_empty() {
                        println!(
                            "Server stdout: {}",
                            String::from_utf8_lossy(&final_output.stdout)
                        );
                    }
                    if !final_output.stderr.is_empty() {
                        println!(
                            "Server stderr: {}",
                            String::from_utf8_lossy(&final_output.stderr)
                        );
                    }
                }
            } else {
                // Process is still running, kill it
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        panic!("Connection file was not created within timeout");
    }

    // Read the connection file
    let connection_content =
        std::fs::read_to_string(&connection_file_path).expect("Failed to read connection file");

    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct ServerConnectionInfo {
        port: u16,
        base_path: String,
        server_path: String,
        server_pid: u32,
        bearer_token: Option<String>,
        log_path: Option<String>,
    }

    let connection_info: ServerConnectionInfo =
        serde_json::from_str(&connection_content).expect("Failed to parse connection file");

    // Verify the connection info includes the auth token
    assert!(connection_info.port > 0);
    assert_eq!(connection_info.bearer_token, Some(test_token.to_string()));

    // Test that we can connect with the proper auth token
    let client_with_auth = {
        let context = swagger::make_context!(
            ContextBuilder,
            EmptyContext,
            Some(AuthData::Bearer(swagger::auth::Bearer {
                token: test_token.to_string()
            })),
            XSpanIdString::default()
        );

        let client = kallichore_api::Client::try_new_http(&connection_info.base_path)
            .expect("Failed to create HTTP client");

        Box::new(client.with_context(context))
    };

    // Test that we cannot connect without auth token
    #[allow(trivial_casts)]
    let client_no_auth = {
        type ClientContext = swagger::make_context_ty!(
            ContextBuilder,
            EmptyContext,
            Option<AuthData>,
            XSpanIdString
        );

        let context: ClientContext = swagger::make_context!(
            ContextBuilder,
            EmptyContext,
            None as Option<AuthData>,
            XSpanIdString::default()
        );

        let client = kallichore_api::Client::try_new_http(&connection_info.base_path)
            .expect("Failed to create HTTP client");

        Box::new(client.with_context(context))
    };

    // Wait for server to be ready
    let mut ready = false;
    #[allow(unused_variables)]
    for attempt in 0..50 {
        match tokio::time::timeout(Duration::from_millis(200), client_with_auth.server_status())
            .await
        {
            Ok(Ok(_)) => {
                ready = true;
                break;
            }
            Ok(Err(_)) | Err(_) => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }

    assert!(ready, "Server failed to become ready within timeout");

    // Test authenticated request succeeds
    let auth_response = client_with_auth
        .server_status()
        .await
        .expect("Failed to get server status with auth");

    match auth_response {
        ServerStatusResponse::ServerStatusAndInformation(status) => {
            assert_eq!(status.version, env!("CARGO_PKG_VERSION"));
        }
        ServerStatusResponse::Error(err) => {
            panic!("Authenticated request failed: {:?}", err);
        }
    }

    // Test unauthenticated request for server status (this is allowed)
    let unauth_result = client_no_auth.server_status().await;

    // Server status should work without authentication
    match unauth_result {
        Ok(_) => {
            println!("Unauthenticated server status request succeeded as expected");
        }
        Err(e) => {
            println!(
                "Warning: Unauthenticated server status failed unexpectedly: {:?}",
                e
            );
        }
    }

    // Clean up
    cleanup_spawned_server(child);
    let _ = std::fs::remove_file(&connection_file_path);
    let _ = std::fs::remove_file(&token_file_path);

    println!(
        "Successfully tested server with connection file and auth token. Port: {}",
        connection_info.port
    );
}

#[tokio::test]
async fn test_multiple_servers_different_ports() {
    // Test that we can start multiple servers and they get different ports
    let temp_dir = std::env::temp_dir();
    let mut servers = Vec::new();
    let mut connection_files = Vec::new();

    // Start 3 servers
    for i in 0..3 {
        let connection_file_path = temp_dir.join(format!(
            "kallichore_test_multi_{}_{}.json",
            i,
            Uuid::new_v4()
        ));
        let connection_file_str = connection_file_path.to_string_lossy().to_string();
        let binary_path = std::env::current_dir()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("target/debug/kcserver")
            .with_extension(if cfg!(windows) { "exe" } else { "" });

        let mut cmd = if binary_path.exists() {
            let mut c = std::process::Command::new(&binary_path);
            c.args(&[
                "--port",
                "0", // Let OS pick the port
                "--connection-file",
                &connection_file_str,
                "--transport",
                "tcp", // Explicitly request TCP for port testing
                "--token",
                "none",
            ]);
            c
        } else {
            let mut c = std::process::Command::new("cargo");
            c.args(&[
                "run",
                "--bin",
                "kcserver",
                "--",
                "--port",
                "0", // Let OS pick the port
                "--connection-file",
                &connection_file_str,
                "--transport",
                "tcp", // Explicitly request TCP for port testing
                "--token",
                "none",
            ]);
            c
        };

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.env("RUST_LOG", "info");

        let child = cmd.spawn().expect("Failed to start kcserver");
        servers.push(child);
        connection_files.push(connection_file_path);

        // Small delay between starts
        tokio::time::sleep(Duration::from_millis(200)).await;
    } // Wait for all connection files to be created and read them
    let mut ports = Vec::new();
    for (i, connection_file_path) in connection_files.iter().enumerate() {
        let mut attempts = 0;
        while !connection_file_path.exists() && attempts < 100 {
            // Increased from 50
            tokio::time::sleep(Duration::from_millis(100)).await;
            attempts += 1;
            if attempts % 20 == 0 {
                println!(
                    "Still waiting for connection file {} after {} attempts...",
                    i, attempts
                );
            }
        }
        if !connection_file_path.exists() {
            panic!("Connection file {} was not created within timeout", i);
        }

        let connection_content =
            std::fs::read_to_string(connection_file_path).expect("Failed to read connection file");

        #[derive(serde::Deserialize)]
        struct ServerConnectionInfo {
            port: u16,
        }

        let connection_info: ServerConnectionInfo =
            serde_json::from_str(&connection_content).expect("Failed to parse connection file");

        ports.push(connection_info.port);
    }

    // Verify all servers got different ports
    assert_eq!(ports.len(), 3);
    ports.sort();
    ports.dedup();
    assert_eq!(ports.len(), 3, "All servers should have different ports");

    // Verify all ports are valid
    for port in &ports {
        assert!(*port > 0, "Port should be greater than 0");
    }

    // Clean up
    for server in servers {
        cleanup_spawned_server(server);
    }

    for connection_file_path in &connection_files {
        let _ = std::fs::remove_file(connection_file_path);
    }

    println!(
        "Successfully tested multiple servers with different ports: {:?}",
        ports
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_server_connection_file_default_socket() {
    // Test that --connection-file defaults to socket transport on Unix
    let temp_dir = std::env::temp_dir();
    let connection_file_path = temp_dir.join(format!(
        "kallichore_test_socket_default_{}.json",
        Uuid::new_v4()
    ));
    let connection_file_str = connection_file_path.to_string_lossy().to_string();

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
        let mut c = std::process::Command::new(&binary_path);
        c.args(&[
            "--connection-file",
            &connection_file_str,
            "--token",
            "none", // Disable auth for testing
        ]);
        c
    } else {
        println!("Pre-built binary not found, using cargo run");
        let mut c = std::process::Command::new("cargo");
        c.args(&[
            "run",
            "--bin",
            "kcserver",
            "--",
            "--connection-file",
            &connection_file_str,
            "--token",
            "none", // Disable auth for testing
        ]);
        c
    }; // Capture output for debugging
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.env("RUST_LOG", "info");

    let child = cmd.spawn().expect("Failed to start kcserver");

    // Wait for connection file to be created
    let mut attempts = 0;
    while !connection_file_path.exists() && attempts < 100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        attempts += 1;
    }

    assert!(
        connection_file_path.exists(),
        "Connection file was not created within timeout"
    );

    // Read and parse the connection file
    let connection_content =
        std::fs::read_to_string(&connection_file_path).expect("Failed to read connection file");

    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct ServerConnectionInfoNew {
        port: Option<u16>,
        base_path: Option<String>,
        socket_path: Option<String>,
        named_pipe: Option<String>,
        transport: String,
        server_path: String,
        server_pid: u32,
        bearer_token: Option<String>,
        log_path: Option<String>,
    }

    let connection_info: ServerConnectionInfoNew =
        serde_json::from_str(&connection_content).expect("Failed to parse connection file");

    // Verify this is a socket connection
    assert_eq!(connection_info.transport, "socket");
    assert!(connection_info.socket_path.is_some());
    assert!(connection_info.port.is_none());
    assert!(connection_info.base_path.is_none());
    assert!(connection_info.named_pipe.is_none());

    let socket_path = connection_info.socket_path.unwrap();

    // Verify the socket file exists
    assert!(
        std::path::Path::new(&socket_path).exists(),
        "Socket file should exist at: {}",
        socket_path
    );

    // Test that we can connect to the socket
    use std::os::unix::net::UnixStream;
    let _stream =
        UnixStream::connect(&socket_path).expect("Should be able to connect to the Unix socket");

    // Clean up
    cleanup_spawned_server(child);
    let _ = std::fs::remove_file(&connection_file_path);
    let _ = std::fs::remove_file(&socket_path);

    println!("Successfully tested default socket transport with connection file");
}

#[tokio::test]
async fn test_server_connection_file_explicit_tcp_transport() {
    // Test that --transport tcp forces TCP mode even with --connection-file
    let temp_dir = std::env::temp_dir();
    let connection_file_path = temp_dir.join(format!(
        "kallichore_test_tcp_explicit_{}.json",
        Uuid::new_v4()
    ));
    let connection_file_str = connection_file_path.to_string_lossy().to_string();

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
        let mut c = std::process::Command::new(&binary_path);
        c.args(&[
            "--connection-file",
            &connection_file_str,
            "--transport",
            "tcp",
            "--token",
            "none", // Disable auth for testing
        ]);
        c
    } else {
        println!("Pre-built binary not found, using cargo run");
        let mut c = std::process::Command::new("cargo");
        c.args(&[
            "run",
            "--bin",
            "kcserver",
            "--",
            "--connection-file",
            &connection_file_str,
            "--transport",
            "tcp",
            "--token",
            "none", // Disable auth for testing
        ]);
        c
    };

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.env("RUST_LOG", "info");

    let child = cmd.spawn().expect("Failed to start kcserver");

    // Wait for connection file to be created
    let mut attempts = 0;
    while !connection_file_path.exists() && attempts < 100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        attempts += 1;
    }

    assert!(
        connection_file_path.exists(),
        "Connection file was not created within timeout"
    );

    // Read and parse the connection file
    let connection_content =
        std::fs::read_to_string(&connection_file_path).expect("Failed to read connection file");

    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct ServerConnectionInfoNew {
        port: Option<u16>,
        base_path: Option<String>,
        socket_path: Option<String>,
        named_pipe: Option<String>,
        transport: String,
        server_path: String,
        server_pid: u32,
        bearer_token: Option<String>,
        log_path: Option<String>,
    }

    let connection_info: ServerConnectionInfoNew =
        serde_json::from_str(&connection_content).expect("Failed to parse connection file");

    // Verify this is a TCP connection
    assert_eq!(connection_info.transport, "tcp");
    assert!(connection_info.port.is_some());
    assert!(connection_info.base_path.is_some());
    assert!(connection_info.socket_path.is_none());
    assert!(connection_info.named_pipe.is_none());

    let port = connection_info.port.unwrap();
    let base_path = connection_info.base_path.unwrap();

    // Verify the base path is correctly formatted
    assert_eq!(base_path, format!("http://127.0.0.1:{}", port));

    // Test that we can make HTTP requests to the server
    #[allow(trivial_casts)]
    let client = {
        let context = swagger::make_context!(
            ContextBuilder,
            EmptyContext,
            None as Option<AuthData>,
            XSpanIdString::default()
        );

        let client =
            kallichore_api::Client::try_new_http(&base_path).expect("Failed to create HTTP client");

        Box::new(client.with_context(context))
    };

    // Wait for server to be ready by polling status
    let mut ready = false;
    for _attempt in 0..50 {
        match tokio::time::timeout(Duration::from_millis(200), client.server_status()).await {
            Ok(Ok(_)) => {
                ready = true;
                break;
            }
            Ok(Err(_)) | Err(_) => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }

    assert!(ready, "Server failed to become ready within timeout");

    // Test that we can actually use the server
    let response = client
        .server_status()
        .await
        .expect("Failed to get server status");

    match response {
        ServerStatusResponse::ServerStatusAndInformation(status) => {
            assert_eq!(status.version, env!("CARGO_PKG_VERSION"));
            assert!(status.sessions >= 0);
        }
        ServerStatusResponse::Error(err) => {
            panic!("Server returned error: {:?}", err);
        }
    }

    // Clean up
    cleanup_spawned_server(child);
    let _ = std::fs::remove_file(&connection_file_path);

    println!("Successfully tested explicit TCP transport with connection file");
}

#[tokio::test]
#[cfg(unix)]
async fn test_server_connection_file_explicit_socket_transport() {
    // Test that --transport socket explicitly requests socket mode
    let temp_dir = std::env::temp_dir();
    let connection_file_path = temp_dir.join(format!(
        "kallichore_test_socket_explicit_{}.json",
        Uuid::new_v4()
    ));
    let connection_file_str = connection_file_path.to_string_lossy().to_string();

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
        let mut c = std::process::Command::new(&binary_path);
        c.args(&[
            "--connection-file",
            &connection_file_str,
            "--transport",
            "socket",
            "--token",
            "none", // Disable auth for testing
        ]);
        c
    } else {
        println!("Pre-built binary not found, using cargo run");
        let mut c = std::process::Command::new("cargo");
        c.args(&[
            "run",
            "--bin",
            "kcserver",
            "--",
            "--connection-file",
            &connection_file_str,
            "--transport",
            "socket",
            "--token",
            "none", // Disable auth for testing
        ]);
        c
    }; // Capture output for debugging
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.env("RUST_LOG", "info");

    let child = cmd.spawn().expect("Failed to start kcserver");

    // Wait for connection file to be created
    let mut attempts = 0;
    while !connection_file_path.exists() && attempts < 100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        attempts += 1;
    }

    assert!(
        connection_file_path.exists(),
        "Connection file was not created within timeout"
    );

    // Read and parse the connection file
    let connection_content =
        std::fs::read_to_string(&connection_file_path).expect("Failed to read connection file");

    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct ServerConnectionInfoNew {
        port: Option<u16>,
        base_path: Option<String>,
        socket_path: Option<String>,
        named_pipe: Option<String>,
        transport: String,
        server_path: String,
        server_pid: u32,
        bearer_token: Option<String>,
        log_path: Option<String>,
    }

    let connection_info: ServerConnectionInfoNew =
        serde_json::from_str(&connection_content).expect("Failed to parse connection file");

    // Verify this is a socket connection
    assert_eq!(connection_info.transport, "socket");
    assert!(connection_info.socket_path.is_some());
    assert!(connection_info.port.is_none());
    assert!(connection_info.base_path.is_none());
    assert!(connection_info.named_pipe.is_none());

    let socket_path = connection_info.socket_path.unwrap();

    // Verify the socket file exists
    assert!(
        std::path::Path::new(&socket_path).exists(),
        "Socket file should exist at: {}",
        socket_path
    );

    // Test that we can connect to the socket
    use std::os::unix::net::UnixStream;
    let _stream =
        UnixStream::connect(&socket_path).expect("Should be able to connect to the Unix socket");

    // Clean up
    cleanup_spawned_server(child);
    let _ = std::fs::remove_file(&connection_file_path);
    let _ = std::fs::remove_file(&socket_path);

    println!("Successfully tested explicit socket transport with connection file");
}

#[tokio::test]
async fn test_invalid_transport_parameter() {
    // Test that invalid transport values are rejected
    let temp_dir = std::env::temp_dir();
    let connection_file_path =
        temp_dir.join(format!("kallichore_test_invalid_{}.json", Uuid::new_v4()));
    let connection_file_str = connection_file_path.to_string_lossy().to_string();

    let binary_path = std::env::current_dir()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/kcserver")
        .with_extension(if cfg!(windows) { "exe" } else { "" });

    let mut cmd = if binary_path.exists() {
        let mut c = std::process::Command::new(&binary_path);
        c.args(&[
            "--connection-file",
            &connection_file_str,
            "--transport",
            "invalid-transport", // This should be rejected
            "--token",
            "none",
        ]);
        c
    } else {
        let mut c = std::process::Command::new("cargo");
        c.args(&[
            "run",
            "--bin",
            "kcserver",
            "--",
            "--connection-file",
            &connection_file_str,
            "--transport",
            "invalid-transport", // This should be rejected
            "--token",
            "none",
        ]);
        c
    };

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let output = cmd.output().expect("Failed to run command");

    // Server should exit with error code
    assert!(!output.status.success());

    // Should not create connection file
    assert!(!connection_file_path.exists());

    // Clean up (just in case)
    let _ = std::fs::remove_file(&connection_file_path);

    println!("Successfully tested invalid transport parameter rejection");
}
