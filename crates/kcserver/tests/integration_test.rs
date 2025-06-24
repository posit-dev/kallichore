//
// integration_test.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//
//

//! Integration tests for Kallichore server
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

    struct UnixSocketTestServer {
        child: std::process::Child,
        socket_path: PathBuf,
        _temp_dir: tempfile::TempDir, // Keep temp dir alive
    }

    impl UnixSocketTestServer {
        async fn start() -> Self {
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

        fn socket_path(&self) -> &std::path::Path {
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
        let create_session_result = create_session_via_unix_http(socket_path, &session_id).await;
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

    async fn create_session_via_unix_http(
        socket_path: &std::path::Path,
        session_id: &str,
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
            "argv": ["python3", "-m", "ipykernel_launcher", "-f", "{{connection_file}}"],
            "working_directory": "{}",
            "env": [],
            "connection_timeout": 3,
            "interrupt_mode": "message",
            "protocol_version": "5.3",
            "run_in_shell": false
        }}"#,
            session_id,
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

    async fn start_session_via_unix_http(
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

    async fn upgrade_channels_via_unix_http(
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

// Global shared test server
static TEST_SERVER: OnceCell<TestServer> = OnceCell::const_new();

async fn get_test_server() -> &'static TestServer {
    TEST_SERVER.get_or_init(TestServer::start).await
}

#[tokio::test]
async fn test_server_starts_and_responds() {
    let server = get_test_server().await;
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

            run_python_kernel_test(&python_cmd).await;
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

async fn run_python_kernel_test(python_cmd: &str) {
    let server = get_test_server().await;
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

    // Connect to the websocket for this session
    let ws_url = format!(
        "ws://localhost:{}/sessions/{}/channels",
        server.port(),
        session_id
    );

    let (ws_stream, _) = connect_async(&ws_url)
        .await
        .expect("Failed to connect to websocket");

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

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
    let message_json = serde_json::to_string(&ws_message).expect("Failed to serialize message");

    println!("Sending kernel_info_request to Python kernel...");
    ws_sender
        .send(Message::Text(message_json))
        .await
        .expect("Failed to send websocket message");

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
    let message_json = serde_json::to_string(&ws_message).expect("Failed to serialize message");

    println!("Sending execute_request to Python kernel...");
    ws_sender
        .send(Message::Text(message_json))
        .await
        .expect("Failed to send websocket message");

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
        match tokio::time::timeout(Duration::from_millis(1000), ws_receiver.next()).await {
            // Reduced from 3 seconds
            Ok(Some(Ok(Message::Text(text)))) => {
                message_count += 1;
                println!("Received message {}: {}", message_count, text);

                // Try to parse as WebsocketMessage
                if let Ok(ws_msg) = serde_json::from_str::<WebsocketMessage>(&text) {
                    match ws_msg {
                        WebsocketMessage::Jupyter(jupyter_msg) => {
                            received_jupyter_messages += 1;
                            println!("  -> Jupyter message type: {}", jupyter_msg.header.msg_type);

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
                                        println!("  🎉 Execution completed with expected output!");
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
                                if let Some(state) = jupyter_msg.content.get("execution_state") {
                                    println!("  -> Kernel state: {}", state);
                                }
                            }
                        }
                        WebsocketMessage::Kernel(kernel_msg) => {
                            received_kernel_messages += 1;
                            println!("  -> Kernel message: {:?}", kernel_msg);
                        }
                    }
                }
            }
            Ok(Some(Ok(Message::Ping(_)))) => {
                // Pings are normal, don't count them
                println!("Received ping message");
            }
            Ok(Some(Ok(Message::Pong(_)))) => {
                println!("Received pong message");
            }
            Ok(Some(Ok(Message::Binary(data)))) => {
                println!("Received binary message ({} bytes)", data.len());
            }
            Ok(Some(Ok(msg))) => {
                println!("Received other message type: {:?}", msg);
            }
            Ok(Some(Err(e))) => {
                println!("Websocket error: {}", e);
                break;
            }
            Ok(None) => {
                println!("Websocket closed");
                break;
            }
            Err(_) => {
                // Timeout is normal if no messages
                println!("Timeout waiting for message");
                continue;
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
    // Properly close the websocket connection
    if let Err(e) = ws_sender.send(Message::Close(None)).await {
        println!("Failed to send close message: {}", e);
    }
}

async fn find_python_executable() -> Option<String> {
    let candidates = vec!["python3", "python"];

    for candidate in candidates {
        match tokio::process::Command::new(candidate)
            .arg("--version")
            .output()
            .await
        {
            Ok(output) if output.status.success() => {
                println!("Found Python at: {}", candidate);
                // If it's a relative command, try to find the full path using `which`
                if !candidate.starts_with('/') {
                    if let Ok(which_output) = tokio::process::Command::new("which")
                        .arg(candidate)
                        .output()
                        .await
                    {
                        if which_output.status.success() {
                            let full_path = String::from_utf8_lossy(&which_output.stdout)
                                .trim()
                                .to_string();
                            if !full_path.is_empty() {
                                println!("Full path for {}: {}", candidate, full_path);
                                return Some(full_path);
                            }
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

    let server = get_test_server().await;
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
}

#[tokio::test]
async fn test_server_starts_with_connection_file() {
    // Create a temporary connection file path
    let temp_dir = std::env::temp_dir();
    let connection_file_path = temp_dir.join(format!(
        "kallichore_test_connection_{}.json",
        Uuid::new_v4()
    ));
    let connection_file_str = connection_file_path.to_string_lossy().to_string();

    // Start server without specifying port (port 0), using connection file
    let binary_path = std::env::current_dir()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/kcserver");

    let mut cmd = if binary_path.exists() {
        let mut c = std::process::Command::new(&binary_path);
        c.args(&[
            "--port",
            "0", // Let OS pick the port
            "--connection-file",
            &connection_file_str,
            "--token",
            "none", // Disable auth for testing
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
            "--token",
            "none", // Disable auth for testing
        ]);
        c
    };

    // Reduce logging noise for faster startup
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    cmd.env("RUST_LOG", "error");

    let mut child = cmd.spawn().expect("Failed to start kcserver");

    // Wait for connection file to be created
    let mut attempts = 0;
    while !connection_file_path.exists() && attempts < 50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        attempts += 1;
    }

    assert!(
        connection_file_path.exists(),
        "Connection file was not created within timeout"
    );

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
    let _ = child.kill();
    let _ = child.wait();
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
    std::fs::write(&token_file_path, test_token).expect("Failed to write token file");

    // Start server without specifying port, using connection file and auth token
    let binary_path = std::env::current_dir()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/kcserver");

    let mut cmd = if binary_path.exists() {
        let mut c = std::process::Command::new(&binary_path);
        c.args(&[
            "--port",
            "0", // Let OS pick the port
            "--connection-file",
            &connection_file_str,
            "--token",
            &token_file_str,
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
            "--token",
            &token_file_str,
        ]);
        c
    };

    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    cmd.env("RUST_LOG", "error");

    let mut child = cmd.spawn().expect("Failed to start kcserver");

    // Wait for connection file to be created
    let mut attempts = 0;
    while !connection_file_path.exists() && attempts < 50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        attempts += 1;
    }

    assert!(
        connection_file_path.exists(),
        "Connection file was not created within timeout"
    );

    // Read the connection file
    let connection_content =
        std::fs::read_to_string(&connection_file_path).expect("Failed to read connection file");

    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct ServerConnectionInfo {
        port: u16,
        base_path: String,
        #[allow(dead_code)]
        server_path: String,
        server_pid: u32,
        bearer_token: Option<String>,
        #[allow(dead_code)]
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
    let _ = child.kill();
    let _ = child.wait();
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
            .join("target/debug/kcserver");

        let mut cmd = if binary_path.exists() {
            let mut c = std::process::Command::new(&binary_path);
            c.args(&[
                "--port",
                "0", // Let OS pick the port
                "--connection-file",
                &connection_file_str,
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
                "--token",
                "none",
            ]);
            c
        };

        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
        cmd.env("RUST_LOG", "error");

        let child = cmd.spawn().expect("Failed to start kcserver");
        servers.push(child);
        connection_files.push(connection_file_path);

        // Small delay between starts
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Wait for all connection files to be created and read them
    let mut ports = Vec::new();
    for connection_file_path in &connection_files {
        let mut attempts = 0;
        while !connection_file_path.exists() && attempts < 50 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            attempts += 1;
        }

        assert!(
            connection_file_path.exists(),
            "Connection file was not created within timeout"
        );

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
    for mut server in servers {
        let _ = server.kill();
        let _ = server.wait();
    }

    for connection_file_path in &connection_files {
        let _ = std::fs::remove_file(connection_file_path);
    }

    println!(
        "Successfully tested multiple servers with different ports: {:?}",
        ports
    );
}
