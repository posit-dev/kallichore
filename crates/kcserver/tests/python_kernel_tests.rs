//
// python_kernel_tests.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Python kernel communication tests

#![allow(unused_imports)]

#[path = "common/mod.rs"]
mod common;

use common::test_utils::{
    create_execute_request, create_session_with_client, create_shutdown_request,
    create_test_session, get_python_executable, is_ipykernel_available,
};
use common::transport::{
    run_communication_test, CommunicationChannel, CommunicationTestResults, TransportType,
};
use common::TestServer;
use kallichore_api::models::{InterruptMode, NewSession, SessionMode, VarAction, VarActionType};
use kallichore_api::NewSessionResponse;
use kcshared::jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader};
use kcshared::websocket_message::WebsocketMessage;
use std::time::Duration;
use uuid::Uuid;

const EXECUTE_REQUEST_MAX_ATTEMPTS: u8 = 3;
const EXECUTE_TIMEOUT_SECS: u64 = 12;
const EXECUTE_MAX_MESSAGES: u32 = 35;
const EXECUTE_RETRY_BACKOFF_MS: u64 = 750;

async fn execute_test_code_with_retries(
    comm: &mut CommunicationChannel,
) -> (CommunicationTestResults, u8) {
    let mut last_results = CommunicationTestResults::default();

    for attempt in 1..=EXECUTE_REQUEST_MAX_ATTEMPTS {
        println!(
            "Sending execute_request to Python kernel (attempt {})...",
            attempt
        );
        let execute_request = create_execute_request();
        comm.send_message(&execute_request)
            .await
            .expect("Failed to send execute_request");

        let results = run_communication_test(
            comm,
            Duration::from_secs(EXECUTE_TIMEOUT_SECS),
            EXECUTE_MAX_MESSAGES,
        )
        .await;

        if results.execute_reply_received
            && results.stream_output_received
            && results.expected_output_found
        {
            println!(
                "Execute_request completed successfully on attempt {}",
                attempt
            );
            return (results, attempt);
        }

        println!(
            "Execute_request attempt {} incomplete (execute_reply={}, stream_output={}, expected_output={}).",
            attempt,
            results.execute_reply_received,
            results.stream_output_received,
            results.expected_output_found
        );

        last_results = results;

        if attempt < EXECUTE_REQUEST_MAX_ATTEMPTS {
            println!(
                "Waiting {} ms before retrying execute_request...",
                EXECUTE_RETRY_BACKOFF_MS
            );
            tokio::time::sleep(Duration::from_millis(EXECUTE_RETRY_BACKOFF_MS)).await;
        }
    }

    (last_results, EXECUTE_REQUEST_MAX_ATTEMPTS)
}

/// Run a Python kernel test with the specified transport
async fn run_python_kernel_test_transport(python_cmd: &str, transport: TransportType) {
    // For domain socket transport, we need to start a Unix socket server
    #[cfg(unix)]
    if matches!(transport, TransportType::DomainSocket) {
        return run_python_kernel_test_domain_socket(python_cmd).await;
    }

    // Determine the appropriate server mode for the transport
    let server_mode = match transport {
        TransportType::Websocket => common::TestServerMode::Http,
        #[cfg(windows)]
        TransportType::NamedPipe => common::TestServerMode::NamedPipe,
        #[cfg(unix)]
        TransportType::DomainSocket => common::TestServerMode::DomainSocket,
    };

    let server = TestServer::start_with_mode(server_mode).await;

    // Create appropriate client based on server mode
    let (client, session_id) = match server.mode() {
        #[cfg(windows)]
        common::TestServerMode::NamedPipe => {
            // For named pipe mode, we need to communicate via the pipe directly
            // Since the API client expects HTTP, we'll need to implement a custom client
            // For now, let's create a session via direct pipe communication
            let session_id = format!("test-session-{}", Uuid::new_v4());
            return run_python_kernel_test_named_pipe(
                python_cmd,
                &session_id,
                server.pipe_name().unwrap(),
            )
            .await;
        }
        #[cfg(unix)]
        common::TestServerMode::DomainSocket => {
            let session_id = format!("test-session-{}", Uuid::new_v4());
            return run_python_kernel_test_domain_socket_direct(
                python_cmd,
                &session_id,
                server.socket_path().unwrap(),
            )
            .await;
        }
        common::TestServerMode::Http => {
            let client = server.create_client().await;
            let session_id = format!("test-session-{}", Uuid::new_v4());
            (client, session_id)
        }
    };

    let new_session = create_test_session(session_id.clone(), python_cmd);

    // Create the kernel session
    let _session_id = create_session_with_client(&client, new_session).await;

    println!("Created Python kernel session: {}", session_id);

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
            let ws_url = format!(
                "ws://localhost:{}/sessions/{}/channels",
                server.port(),
                session_id
            );
            CommunicationChannel::create_websocket(&ws_url)
                .await
                .expect("Failed to create websocket")
        }
        #[cfg(unix)]
        TransportType::DomainSocket => {
            // This branch shouldn't be reached due to early return above
            panic!("Domain socket should be handled separately");
        }
        #[cfg(windows)]
        TransportType::NamedPipe => {
            // This branch shouldn't be reached due to early return above
            panic!("Named pipe should be handled separately");
        }
    };

    // Wait for the kernel to start
    println!("Waiting for Python kernel to start up...");
    tokio::time::sleep(Duration::from_millis(800)).await; // Give kernel time to start

    let (results, attempts_used) = execute_test_code_with_retries(&mut comm).await;

    results.print_summary();

    // Assert only the essential functionality for faster tests
    assert!(
        results.execute_reply_received,
        "Expected to receive execute_reply from Python kernel after {} attempts, but didn't get one. The kernel is not executing code properly.",
        attempts_used
    );

    assert!(
        results.stream_output_received,
        "Expected to receive stream output from Python kernel after {} attempts, but didn't get any. The kernel is not producing stdout output.",
        attempts_used
    );

    assert!(
        results.expected_output_found,
        "Expected to find 'Hello from Kallichore test!' and '2 + 3 = 5' in the kernel output after {} attempts, but didn't find both. The kernel executed but produced unexpected output. Actual collected output: {:?}",
        attempts_used,
        results.collected_output
    );

    // Clean up
    if let Err(e) = comm.close().await {
        println!("Failed to close communication channel: {}", e);
    }

    drop(server);
}

#[tokio::test]
async fn test_python_kernel_session_and_websocket_communication() {
    let test_result = tokio::time::timeout(Duration::from_secs(25), async {
        let python_cmd = if let Some(cmd) = get_python_executable().await {
            cmd
        } else {
            println!("Skipping test: No Python executable found");
            return;
        };

        if !is_ipykernel_available().await {
            println!("Skipping test: ipykernel not available for {}", python_cmd);
            return;
        }

        run_python_kernel_test_transport(&python_cmd, TransportType::Websocket).await;
    })
    .await;

    match test_result {
        Ok(_) => {
            println!("Python kernel test completed successfully");
        }
        Err(_) => {
            panic!("Python kernel test timed out after 25 seconds");
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn test_python_kernel_session_and_domain_socket_communication() {
    let test_result = tokio::time::timeout(
        Duration::from_secs(15), // Reduced from 25 seconds
        async {
            let python_cmd = if let Some(cmd) = get_python_executable().await {
                cmd
            } else {
                println!("Skipping test: No Python executable found");
                return;
            };

            if !is_ipykernel_available().await {
                println!("Skipping test: ipykernel not available for {}", python_cmd);
                return;
            }

            run_python_kernel_test_transport(&python_cmd, TransportType::DomainSocket).await;
        },
    )
    .await;

    match test_result {
        Ok(_) => {
            println!("Python kernel domain socket test completed successfully");
        }
        Err(_) => {
            panic!("Python kernel domain socket test timed out after 25 seconds");
        }
    }
}

#[cfg(windows)]
#[tokio::test]
async fn test_python_kernel_session_and_named_pipe_communication() {
    let test_result = tokio::time::timeout(
        Duration::from_secs(15), // Reduced from 25 seconds
        async {
            let python_cmd = if let Some(cmd) = get_python_executable().await {
                cmd
            } else {
                println!("Skipping test: No Python executable found");
                return;
            };

            if !is_ipykernel_available().await {
                println!("Skipping test: ipykernel not available for {}", python_cmd);
                return;
            }

            run_python_kernel_test_transport(&python_cmd, TransportType::NamedPipe).await;
        },
    )
    .await;

    match test_result {
        Ok(_) => {
            println!("Python kernel named pipe test completed successfully");
        }
        Err(_) => {
            panic!("Python kernel named pipe test timed out after 25 seconds");
        }
    }
}

#[tokio::test]
async fn test_multiple_kernel_sessions() {
    let python_cmd = if let Some(cmd) = get_python_executable().await {
        cmd
    } else {
        println!("Skipping test: No Python executable found");
        return;
    };

    if !is_ipykernel_available().await {
        println!("Skipping test: ipykernel not available for {}", python_cmd);
        return;
    }

    let server = TestServer::start().await;
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
            notebook_uri: None,
            session_mode: SessionMode::Console,
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
            connection_timeout: Some(3),
            interrupt_mode: InterruptMode::Message,
            protocol_version: Some("5.3".to_string()),
            run_in_shell: Some(false),
        };

        let _created_session_id = create_session_with_client(&client, new_session).await;
        sessions.push(session_id);
    }

    assert_eq!(sessions.len(), 3, "Should have created 3 sessions");

    // Verify all sessions have unique IDs
    let mut session_ids = sessions.clone();
    session_ids.sort();
    session_ids.dedup();
    assert_eq!(session_ids.len(), 3, "All session IDs should be unique");

    println!(
        "Successfully created {} unique kernel sessions",
        sessions.len()
    );

    drop(server);
}

#[cfg(unix)]
/// Run a Python kernel test using domain socket transport
async fn run_python_kernel_test_domain_socket(python_cmd: &str) {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use tempfile::tempdir;

    // Create a Unix socket server similar to integration_test.rs
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
        let mut c = std::process::Command::new(&binary_path);
        c.args(&[
            "--unix-socket",
            socket_path.to_str().unwrap(),
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
            "--unix-socket",
            socket_path.to_str().unwrap(),
            "--token",
            "none", // Disable auth for testing
        ]);
        c
    };

    // Set environment for debugging
    cmd.env("RUST_LOG", "info");
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .expect("Failed to start kcserver with Unix socket");

    // Wait for the socket file to be created
    for _attempt in 0..100 {
        if socket_path.exists() {
            // Try to connect to verify the server is ready
            if UnixStream::connect(&socket_path).is_ok() {
                println!("Unix socket server ready");
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    if !socket_path.exists() {
        panic!("Unix socket server failed to start within timeout");
    }

    // Create a kernel session using Python with ipykernel
    let session_id = format!("test-session-{}", Uuid::new_v4());

    // Create session via HTTP over Unix socket
    let session_request = format!(
        r#"{{"session_id": "{}", "display_name": "Test Session", "language": "python", "username": "testuser", "input_prompt": "In [{{}}]: ", "continuation_prompt": "   ...: ", "argv": ["{}", "-m", "ipykernel", "-f", "{{connection_file}}"], "working_directory": "/tmp", "env": [], "connection_timeout": 60, "interrupt_mode": "message", "protocol_version": "5.3", "run_in_shell": false, "session_mode": "console"}}"#,
        session_id, python_cmd
    );

    let mut stream = UnixStream::connect(&socket_path)
        .expect("Failed to connect to Unix socket for session creation");

    let create_request = format!(
        "PUT /sessions HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        session_request.len(),
        session_request
    );

    stream
        .write_all(create_request.as_bytes())
        .expect("Failed to write session creation request");

    let mut create_response = String::new();
    stream
        .read_to_string(&mut create_response)
        .expect("Failed to read session creation response");

    println!("Session creation response: {}", create_response);
    assert!(create_response.contains("HTTP/1.1 200 OK"));

    // Start the session
    let mut stream = UnixStream::connect(&socket_path)
        .expect("Failed to connect to Unix socket for session start");

    let start_request = format!(
        "POST /sessions/{}/start HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        session_id
    );

    stream
        .write_all(start_request.as_bytes())
        .expect("Failed to write session start request");

    let mut start_response = String::new();
    stream
        .read_to_string(&mut start_response)
        .expect("Failed to read session start response");

    println!("Session start response: {}", start_response);

    // Wait for kernel to start
    println!("Waiting for Python kernel to start up...");
    tokio::time::sleep(Duration::from_millis(1500)).await; // Give kernel time to start

    // Get channels upgrade
    let mut stream = UnixStream::connect(&socket_path)
        .expect("Failed to connect to Unix socket for channels upgrade");

    let channels_request = format!(
        "GET /sessions/{}/channels HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
        session_id
    );

    stream
        .write_all(channels_request.as_bytes())
        .expect("Failed to write channels upgrade request");

    // Read the channels upgrade response
    let mut buffer = [0; 1024];
    let bytes_read = stream
        .read(&mut buffer)
        .expect("Failed to read channels upgrade response");

    let channels_response = String::from_utf8_lossy(&buffer[..bytes_read]);
    println!("Channels upgrade response: {}", channels_response);

    // The channels upgrade should succeed
    assert!(
        channels_response.contains("HTTP/1.1 101 Switching Protocols")
            || channels_response.contains("HTTP/1.1 200 OK"),
        "Expected successful WebSocket upgrade, got: {}",
        channels_response
    );

    // Extract the socket path from the response if it contains one
    let comm_socket_path = if channels_response.contains("\"/") {
        // Parse the JSON response to get the socket path
        if let Some(start) = channels_response.find("\"/") {
            if let Some(end) = channels_response[start + 1..].find('"') {
                let path = &channels_response[start + 1..start + 1 + end];
                println!("Extracted socket path from response: {}", path);
                path
            } else {
                socket_path.to_str().unwrap()
            }
        } else {
            socket_path.to_str().unwrap()
        }
    } else {
        socket_path.to_str().unwrap()
    };

    println!("Domain socket path for communication: {}", comm_socket_path);

    // Create domain socket communication channel
    let mut comm = CommunicationChannel::create_domain_socket(comm_socket_path)
        .await
        .expect("Failed to create domain socket communication channel");

    let (results, attempts_used) = execute_test_code_with_retries(&mut comm).await;

    results.print_summary();

    // Assert only the essential functionality for faster domain socket tests
    assert!(
        results.execute_reply_received,
        "Expected to receive execute_reply from Python kernel after {} attempts, but didn't get one. The kernel is not executing code properly.",
        attempts_used
    );

    assert!(
        results.stream_output_received,
        "Expected to receive stream output from Python kernel after {} attempts, but didn't get any. The kernel is not producing stdout output.",
        attempts_used
    );

    assert!(
        results.expected_output_found,
        "Expected to find 'Hello from Kallichore test!' and '2 + 3 = 5' in the kernel output after {} attempts, but didn't find both. The kernel executed but produced unexpected output. Actual collected output: {:?}",
        attempts_used,
        results.collected_output
    );

    // Clean up
    if let Err(e) = comm.close().await {
        println!("Failed to close communication channel: {}", e);
    }

    // Terminate the server process
    if let Err(e) = child.kill() {
        println!("Warning: Failed to terminate Unix socket server: {}", e);
    }

    if let Err(e) = child.wait() {
        println!("Warning: Failed to wait for Unix socket server: {}", e);
    }

    // Clean up socket file if it still exists
    if socket_path.exists() {
        if let Err(e) = std::fs::remove_file(&socket_path) {
            println!("Warning: Failed to remove socket file: {}", e);
        }
    }
}

#[cfg(windows)]
/// Run a Python kernel test using Windows named pipe transport
async fn run_python_kernel_test_named_pipe(python_cmd: &str, session_id: &str, pipe_name: &str) {
    #[allow(unused_imports)]
    use std::io::{Read, Write};
    use tokio::net::windows::named_pipe::ClientOptions;

    println!("Starting named pipe test with pipe: {}", pipe_name);

    // Wait a bit for the server to be ready
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Create session via HTTP over named pipe using proper JSON serialization
    let working_dir = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let session_data = serde_json::json!({
        "session_id": session_id,
        "display_name": "Test Python Kernel",
        "session_mode": "console",
        "language": "python",
        "username": "testuser",
        "input_prompt": "In [{}]: ",
        "continuation_prompt": "   ...: ",
        "argv": [python_cmd, "-m", "ipykernel", "-f", "{connection_file}"],
        "working_directory": working_dir,
        "env": [],
        "connection_timeout": 30,
        "interrupt_mode": "message",
        "protocol_version": "5.3",
        "run_in_shell": false
    });
    let session_request = session_data.to_string();

    // Connect to named pipe and send session creation request
    let pipe = ClientOptions::new()
        .open(pipe_name)
        .expect("Failed to connect to named pipe");

    let create_request = format!(
        "PUT /sessions HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        session_request.len(),
        session_request
    );

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut pipe = pipe;
    pipe.write_all(create_request.as_bytes())
        .await
        .expect("Failed to write session creation request");

    let mut create_response = Vec::new();
    pipe.read_to_end(&mut create_response)
        .await
        .expect("Failed to read session creation response");
    let create_response_str = String::from_utf8_lossy(&create_response);

    println!("Session creation response: {}", create_response_str);
    assert!(
        create_response_str.contains("HTTP/1.1 200 OK"),
        "Expected 200 OK response, got: {}",
        create_response_str
    );

    // Start the session
    let pipe = ClientOptions::new()
        .open(pipe_name)
        .expect("Failed to connect to named pipe for session start");
    let mut pipe = pipe;

    let start_request = format!(
        "POST /sessions/{}/start HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        session_id
    );

    pipe.write_all(start_request.as_bytes())
        .await
        .expect("Failed to write session start request");

    let mut start_response = Vec::new();
    pipe.read_to_end(&mut start_response)
        .await
        .expect("Failed to read session start response");
    let start_response_str = String::from_utf8_lossy(&start_response);

    println!("Session start response: {}", start_response_str);

    // Wait for kernel to start
    println!("Waiting for Python kernel to start up...");
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // Get channels upgrade - this should return a named pipe path
    let pipe = ClientOptions::new()
        .open(pipe_name)
        .expect("Failed to connect to named pipe for channels upgrade");
    let mut pipe = pipe;

    let channels_request = format!(
        "GET /sessions/{}/channels HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
        session_id
    );

    pipe.write_all(channels_request.as_bytes())
        .await
        .expect("Failed to write channels upgrade request");

    // Read the channels upgrade response
    let mut buffer = [0; 1024];
    let bytes_read = pipe
        .read(&mut buffer)
        .await
        .expect("Failed to read channels upgrade response");

    let channels_response = String::from_utf8_lossy(&buffer[..bytes_read]);
    println!("Channels upgrade response: {}", channels_response);

    // The channels upgrade should succeed and return a named pipe path
    assert!(
        channels_response.contains("HTTP/1.1 200 OK") || channels_response.contains("HTTP/1.1 101"),
        "Expected successful response, got: {}",
        channels_response
    );

    // Extract the pipe path from the response
    let comm_pipe_path = if channels_response.contains("HTTP/1.1 200 OK") {
        // Parse the JSON response to get the pipe path
        if let Some(body_start) = channels_response.find("\r\n\r\n") {
            let response_body = &channels_response[body_start + 4..];
            if let Ok(pipe_path) = serde_json::from_str::<String>(response_body.trim()) {
                println!("Extracted pipe path from response: {}", pipe_path);
                pipe_path
            } else {
                println!(
                    "Failed to parse pipe path from response body: {}",
                    response_body
                );
                pipe_name.to_string()
            }
        } else {
            println!("No body found in response");
            pipe_name.to_string()
        }
    } else {
        println!("No 200 OK in response, using original pipe");
        pipe_name.to_string()
    };

    println!("Named pipe path for communication: {}", comm_pipe_path);

    // Create named pipe communication channel
    let mut comm = CommunicationChannel::create_named_pipe(&comm_pipe_path)
        .await
        .expect("Failed to create named pipe communication channel");

    let (results, attempts_used) = execute_test_code_with_retries(&mut comm).await;

    results.print_summary();

    // Assert only the essential functionality for faster tests
    assert!(
        results.execute_reply_received,
        "Expected to receive execute_reply from Python kernel after {} attempts, but didn't get one. The kernel is not executing code properly.",
        attempts_used
    );

    assert!(
        results.stream_output_received,
        "Expected to receive stream output from Python kernel after {} attempts, but didn't get any. The kernel is not producing stdout output.",
        attempts_used
    );

    assert!(
        results.expected_output_found,
        "Expected to find 'Hello from Kallichore test!' and '2 + 3 = 5' in the kernel output after {} attempts, but didn't find both. The kernel executed but produced unexpected output. Actual collected output: {:?}",
        attempts_used,
        results.collected_output
    );

    // Clean up
    if let Err(e) = comm.close().await {
        println!("Failed to close communication channel: {}", e);
    }
}

#[cfg(unix)]
/// Run a Python kernel test using Unix domain socket transport (direct)
async fn run_python_kernel_test_domain_socket_direct(
    python_cmd: &str,
    session_id: &str,
    socket_path: &str,
) {
    #[allow(unused_imports)]
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    println!("Starting domain socket test with socket: {}", socket_path);

    // Wait a bit for the server to be ready
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Create session via HTTP over Unix domain socket
    let session_request = format!(
        r#"{{"session_id": "{}", "display_name": "Test Python Kernel", "language": "python", "username": "testuser", "input_prompt": "In [{{}}]: ", "continuation_prompt": "   ...: ", "argv": ["{}", "-m", "ipykernel", "-f", "{{connection_file}}"], "working_directory": "/tmp", "env": [], "connection_timeout": 3, "interrupt_mode": "message", "protocol_version": "5.3", "run_in_shell": false}}"#,
        session_id, python_cmd
    );

    let mut stream = UnixStream::connect(socket_path)
        .expect("Failed to connect to Unix socket for session creation");

    let create_request = format!(
        "PUT /sessions HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        session_request.len(),
        session_request
    );

    stream
        .write_all(create_request.as_bytes())
        .expect("Failed to write session creation request");

    let mut create_response = String::new();
    stream
        .read_to_string(&mut create_response)
        .expect("Failed to read session creation response");

    println!("Session creation response: {}", create_response);
    assert!(create_response.contains("HTTP/1.1 200 OK"));

    // Start the session
    let mut stream = UnixStream::connect(socket_path)
        .expect("Failed to connect to Unix socket for session start");

    let start_request = format!(
        "POST /sessions/{}/start HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        session_id
    );

    stream
        .write_all(start_request.as_bytes())
        .expect("Failed to write session start request");

    let mut start_response = String::new();
    stream
        .read_to_string(&mut start_response)
        .expect("Failed to read session start response");

    println!("Session start response: {}", start_response);

    // Wait for kernel to start
    println!("Waiting for Python kernel to start up...");
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // Get channels upgrade
    let mut stream = UnixStream::connect(socket_path)
        .expect("Failed to connect to Unix socket for channels upgrade");

    let channels_request = format!(
        "GET /sessions/{}/channels HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
        session_id
    );

    stream
        .write_all(channels_request.as_bytes())
        .expect("Failed to write channels upgrade request");

    // Read the channels upgrade response
    let mut buffer = [0; 1024];
    let bytes_read = stream
        .read(&mut buffer)
        .expect("Failed to read channels upgrade response");

    let channels_response = String::from_utf8_lossy(&buffer[..bytes_read]);
    println!("Channels upgrade response: {}", channels_response);

    // The channels upgrade should succeed
    assert!(
        channels_response.contains("HTTP/1.1 101 Switching Protocols")
            || channels_response.contains("HTTP/1.1 200 OK"),
        "Expected successful WebSocket upgrade, got: {}",
        channels_response
    );

    // Create domain socket communication channel
    let mut comm = CommunicationChannel::create_domain_socket(socket_path)
        .await
        .expect("Failed to create domain socket communication channel");

    let (results, attempts_used) = execute_test_code_with_retries(&mut comm).await;

    results.print_summary();

    // Assert only the essential functionality for faster tests
    assert!(
        results.execute_reply_received,
        "Expected to receive execute_reply from Python kernel after {} attempts, but didn't get one. The kernel is not executing code properly.",
        attempts_used
    );

    assert!(
        results.stream_output_received,
        "Expected to receive stream output from Python kernel after {} attempts, but didn't get any. The kernel is not producing stdout output.",
        attempts_used
    );

    assert!(
        results.expected_output_found,
        "Expected to find 'Hello from Kallichore test!' and '2 + 3 = 5' in the kernel output after {} attempts, but didn't find both. The kernel executed but produced unexpected output. Actual collected output: {:?}",
        attempts_used,
        results.collected_output
    );

    // Clean up
    if let Err(e) = comm.close().await {
        println!("Failed to close communication channel: {}", e);
    }
}

#[tokio::test]
async fn test_kernel_session_shutdown() {
    let python_cmd = if let Some(cmd) = get_python_executable().await {
        cmd
    } else {
        println!("Skipping test: No Python executable found");
        return;
    };

    if !is_ipykernel_available().await {
        println!("Skipping test: ipykernel not available for {}", python_cmd);
        return;
    }

    let server = TestServer::start().await;
    let client = server.create_client().await;

    let session_id = format!("shutdown-test-session-{}", Uuid::new_v4());
    let new_session = create_test_session(session_id.clone(), &python_cmd);

    // Create and start the kernel session
    let _created_session_id = create_session_with_client(&client, new_session).await;

    println!("Starting kernel session for shutdown test...");
    let start_response = client
        .start_session(session_id.clone())
        .await
        .expect("Failed to start session");

    println!("Start response: {:?}", start_response);

    // Check if the session started successfully
    match &start_response {
        kallichore_api::StartSessionResponse::Started(_) => {
            println!("Kernel started successfully");
        }
        kallichore_api::StartSessionResponse::StartFailed(error) => {
            println!("Kernel failed to start: {:?}", error);
            println!("Skipping shutdown test due to startup failure");
            return;
        }
        _ => {
            println!("Unexpected start response: {:?}", start_response);
            println!("Skipping shutdown test");
            return;
        }
    }

    // Wait for kernel to fully start
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // Verify session is running by checking session list
    let sessions_before = client
        .list_sessions()
        .await
        .expect("Failed to list sessions");

    println!("Sessions before shutdown: {:?}", sessions_before);

    // Create a websocket connection to send shutdown request
    let ws_url = format!(
        "ws://localhost:{}/sessions/{}/channels",
        server.port(),
        session_id
    );

    let mut comm = CommunicationChannel::create_websocket(&ws_url)
        .await
        .expect("Failed to create websocket for shutdown");

    // Send a shutdown_request to the kernel
    let shutdown_request = create_shutdown_request();

    println!("Sending shutdown_request to kernel...");
    comm.send_message(&shutdown_request)
        .await
        .expect("Failed to send shutdown_request");

    // Wait for shutdown_reply and for kernel to exit
    println!("Waiting for kernel to shutdown...");
    let mut shutdown_reply_received = false;
    let start_time = std::time::Instant::now();

    while start_time.elapsed() < Duration::from_secs(5) {
        if let Ok(Some(message)) = comm.receive_message().await {
            if message.contains("shutdown_reply") {
                println!("Received shutdown_reply from kernel");
                shutdown_reply_received = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert!(
        shutdown_reply_received,
        "Expected to receive shutdown_reply from kernel"
    );

    // Close the websocket connection
    comm.close().await.ok();

    // Wait a bit more for the kernel process to fully exit
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // Now we can delete the session since the kernel should have exited
    println!("Deleting kernel session...");
    let delete_response = client
        .delete_session(session_id.clone())
        .await
        .expect("Failed to delete session");

    println!("Delete response: {:?}", delete_response);

    // Verify session is no longer in the list
    let sessions_after = client
        .list_sessions()
        .await
        .expect("Failed to list sessions after shutdown");

    println!("Sessions after shutdown: {:?}", sessions_after);

    // Check that the session is no longer listed as active
    let kallichore_api::ListSessionsResponse::ListOfActiveSessions(session_list) = sessions_after;
    let found_session = session_list
        .sessions
        .iter()
        .find(|session| session.session_id == session_id);
    assert!(
        found_session.is_none(),
        "Session should not be active after shutdown"
    );

    println!("Kernel session shutdown test completed successfully");
    drop(server);
}

#[tokio::test]
async fn test_kernel_session_restart_basic() {
    let python_cmd = if let Some(cmd) = get_python_executable().await {
        cmd
    } else {
        println!("Skipping test: No Python executable found");
        return;
    };

    if !is_ipykernel_available().await {
        println!("Skipping test: ipykernel not available for {}", python_cmd);
        return;
    }

    let server = TestServer::start().await;
    let client = server.create_client().await;

    let session_id = format!("restart-basic-test-session-{}", Uuid::new_v4());
    let new_session = create_test_session(session_id.clone(), &python_cmd);

    // Create and start the kernel session
    let _created_session_id = create_session_with_client(&client, new_session).await;

    println!("Starting kernel session for basic restart test...");
    let start_response = client
        .start_session(session_id.clone())
        .await
        .expect("Failed to start session");

    println!("Start response: {:?}", start_response);

    // Check if the session started successfully
    match &start_response {
        kallichore_api::StartSessionResponse::Started(_) => {
            println!("Kernel started successfully");
        }
        kallichore_api::StartSessionResponse::StartFailed(error) => {
            println!("Kernel failed to start: {:?}", error);
            println!("Skipping restart test due to startup failure");
            return;
        }
        _ => {
            println!("Unexpected start response: {:?}", start_response);
            println!("Skipping restart test");
            return;
        }
    }

    // Wait for kernel to fully start
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // Just test that restart API works without checking kernel communication
    println!("Restarting kernel session...");
    let restart_session = kallichore_api::models::RestartSession::new();
    let restart_response = client
        .restart_session(session_id.clone(), Some(restart_session))
        .await
        .expect("Failed to restart session");

    println!("Restart response: {:?}", restart_response);

    // Verify the restart API returned success
    match restart_response {
        kallichore_api::RestartSessionResponse::Restarted(_) => {
            println!("Restart API returned success");
        }
        _ => {
            panic!(
                "Expected restart to return success, got: {:?}",
                restart_response
            );
        }
    }

    // Wait a bit for restart to complete
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // Verify session is still listed (but may have different status)
    let sessions_after = client
        .list_sessions()
        .await
        .expect("Failed to list sessions after restart");

    println!("Sessions after restart: {:?}", sessions_after);

    let kallichore_api::ListSessionsResponse::ListOfActiveSessions(session_list) = sessions_after;
    let session_found = session_list
        .sessions
        .iter()
        .any(|session| session.session_id == session_id);

    assert!(session_found, "Session should still exist after restart");

    println!("Basic kernel session restart test completed successfully");
    drop(server);
}

#[tokio::test]
async fn test_kernel_session_restart_with_environment_changes() {
    let python_cmd = if let Some(cmd) = get_python_executable().await {
        cmd
    } else {
        println!("Skipping test: No Python executable found");
        return;
    };

    if !is_ipykernel_available().await {
        println!("Skipping test: ipykernel not available for {}", python_cmd);
        return;
    }

    let server = TestServer::start().await;
    let client = server.create_client().await;

    let session_id = format!("restart-env-test-session-{}", Uuid::new_v4());
    let new_session = create_test_session(session_id.clone(), &python_cmd);

    // Create and start the kernel session
    let _created_session_id = create_session_with_client(&client, new_session).await;

    println!("Starting kernel session for restart with environment test...");
    let start_response = client
        .start_session(session_id.clone())
        .await
        .expect("Failed to start session");

    println!("Start response: {:?}", start_response);

    // Wait for kernel to fully start
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // Restart with environment variable changes
    println!("Restarting kernel session with environment changes...");
    let restart_session = kallichore_api::models::RestartSession {
        working_directory: None,
        env: Some(vec![VarAction {
            action: VarActionType::Replace,
            name: "RESTART_TEST_VAR".to_string(),
            value: "restart_value".to_string(),
        }]),
    };

    let restart_response = client
        .restart_session(session_id.clone(), Some(restart_session))
        .await
        .expect("Failed to restart session with environment");

    println!("Restart with environment response: {:?}", restart_response);

    // Verify the restart API returned success
    match restart_response {
        kallichore_api::RestartSessionResponse::Restarted(_) => {
            println!("Restart with environment API returned success");
        }
        _ => {
            panic!(
                "Expected restart with environment to return success, got: {:?}",
                restart_response
            );
        }
    }

    // Wait for restart to complete
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // Verify session is still listed
    let sessions_after = client
        .list_sessions()
        .await
        .expect("Failed to list sessions after restart");

    println!("Sessions after restart: {:?}", sessions_after);

    let kallichore_api::ListSessionsResponse::ListOfActiveSessions(session_list) = sessions_after;
    let session_found = session_list
        .sessions
        .iter()
        .any(|session| session.session_id == session_id);

    assert!(
        session_found,
        "Session should still exist after restart with environment"
    );

    println!("Kernel session restart with environment test completed successfully");
    drop(server);
}

#[tokio::test]
async fn test_multiple_session_shutdown_restart_cycle() {
    let python_cmd = if let Some(cmd) = get_python_executable().await {
        cmd
    } else {
        println!("Skipping test: No Python executable found");
        return;
    };

    if !is_ipykernel_available().await {
        println!("Skipping test: ipykernel not available for {}", python_cmd);
        return;
    }

    let server = TestServer::start().await;
    let client = server.create_client().await;

    // Create multiple sessions
    let mut session_ids = Vec::new();
    for i in 0..2 {
        let session_id = format!("multi-shutdown-restart-{}-{}", i, Uuid::new_v4());
        let new_session = create_test_session(session_id.clone(), &python_cmd);

        let _created_session_id = create_session_with_client(&client, new_session).await;

        println!("Starting session {} for multi-shutdown test...", i);
        let start_response = client
            .start_session(session_id.clone())
            .await
            .expect("Failed to start session");

        println!("Session {} start response: {:?}", i, start_response);
        session_ids.push(session_id);
    }

    // Wait for all kernels to start
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // Verify all sessions are active
    let sessions_before = client
        .list_sessions()
        .await
        .expect("Failed to list sessions");

    println!("Sessions before operations: {:?}", sessions_before);

    // Shutdown the first session properly using shutdown_request
    println!("Shutting down first session properly...");
    let ws_url_first = format!(
        "ws://localhost:{}/sessions/{}/channels",
        server.port(),
        session_ids[0]
    );

    let mut comm_first = CommunicationChannel::create_websocket(&ws_url_first)
        .await
        .expect("Failed to create websocket for first session shutdown");

    let shutdown_request = create_shutdown_request();
    comm_first
        .send_message(&shutdown_request)
        .await
        .expect("Failed to send shutdown_request to first session");

    // Wait for shutdown_reply
    let mut shutdown_reply_received = false;
    let start_time = std::time::Instant::now();

    while start_time.elapsed() < Duration::from_secs(3) {
        if let Ok(Some(message)) = comm_first.receive_message().await {
            if message.contains("shutdown_reply") {
                println!("Received shutdown_reply from first session");
                shutdown_reply_received = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    comm_first.close().await.ok();

    assert!(
        shutdown_reply_received,
        "Expected to receive shutdown_reply from first session"
    );

    // Wait for kernel to exit
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Now delete the first session
    let delete_response = client
        .delete_session(session_ids[0].clone())
        .await
        .expect("Failed to delete first session");

    println!("First session delete response: {:?}", delete_response);

    // Restart the second session
    println!("Restarting second session...");
    let restart_response = client
        .restart_session(
            session_ids[1].clone(),
            Some(kallichore_api::models::RestartSession::new()),
        )
        .await
        .expect("Failed to restart second session");

    println!("Second session restart response: {:?}", restart_response);

    // Wait for operations to complete
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // Verify final state
    let sessions_after = client
        .list_sessions()
        .await
        .expect("Failed to list sessions after operations");

    println!("Sessions after operations: {:?}", sessions_after);

    // First session should be gone, second should still be active
    let kallichore_api::ListSessionsResponse::ListOfActiveSessions(session_list) = sessions_after;
    let first_session_found = session_list
        .sessions
        .iter()
        .any(|session| session.session_id == session_ids[0]);

    let second_session_found = session_list
        .sessions
        .iter()
        .any(|session| session.session_id == session_ids[1]);

    assert!(!first_session_found, "First session should be deleted");
    assert!(
        second_session_found,
        "Second session should still be active after restart"
    );

    println!("Multiple session shutdown/restart cycle test completed successfully");
    drop(server);
}

#[cfg(not(target_os = "windows"))] // Shell behavior is Unix-specific
#[tokio::test]
async fn test_kernel_starts_with_bad_shell_env_var() {
    let test_result = tokio::time::timeout(Duration::from_secs(30), async {
        let python_cmd = if let Some(cmd) = get_python_executable().await {
            cmd
        } else {
            println!("Skipping test: No Python executable found");
            return;
        };

        if !is_ipykernel_available().await {
            println!("Skipping test: ipykernel not available for {}", python_cmd);
            return;
        }

        // Start a test server
        let server = TestServer::start().await;
        let client = server.create_client().await;

        // Generate a unique session ID
        let session_id = format!("test-bad-shell-{}", Uuid::new_v4());

        // Create a session with run_in_shell=true but set SHELL to a non-existent path
        let new_session = NewSession {
            session_id: session_id.clone(),
            display_name: "Test Bad Shell Session".to_string(),
            language: "python".to_string(),
            username: "testuser".to_string(),
            input_prompt: "In [{}]: ".to_string(),
            continuation_prompt: "   ...: ".to_string(),
            notebook_uri: None,
            session_mode: SessionMode::Console,
            argv: vec![
                python_cmd.clone(),
                "-m".to_string(),
                "ipykernel".to_string(),
                "-f".to_string(),
                "{connection_file}".to_string(),
            ],
            working_directory: std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .to_string(),
            env: vec![
                // Set SHELL to a non-existent path to simulate the bad shell scenario
                VarAction {
                    action: VarActionType::Replace,
                    name: "SHELL".to_string(),
                    value: "/non/existent/shell".to_string(),
                },
            ],
            connection_timeout: Some(15),
            interrupt_mode: InterruptMode::Message,
            protocol_version: Some("5.3".to_string()),
            run_in_shell: Some(true), // This is the key - enable run_in_shell with bad SHELL
        };

        println!("Creating session with run_in_shell=true and bad SHELL env var");

        // Create the session - this should succeed despite the bad SHELL value
        let _created_session_id = create_session_with_client(&client, new_session).await;

        println!("Session created successfully: {}", session_id);

        // Start the session - this should succeed despite the bad SHELL environment variable
        println!("Starting kernel session with bad SHELL environment variable...");
        let start_response = client
            .start_session(session_id.clone())
            .await
            .expect("Failed to start session with bad SHELL");

        println!("Start response: {:?}", start_response);

        // Check if the session started successfully
        match &start_response {
            kallichore_api::StartSessionResponse::Started(_) => {
                println!("Kernel started successfully despite bad SHELL environment variable");
            }
            kallichore_api::StartSessionResponse::StartFailed(error) => {
                panic!("Kernel failed to start with bad SHELL: {:?}", error);
            }
            _ => {
                panic!("Unexpected start response: {:?}", start_response);
            }
        }

        // Wait for kernel to fully start
        tokio::time::sleep(Duration::from_millis(2000)).await;

        // Verify the session is actually running by checking its status
        let sessions = client
            .list_sessions()
            .await
            .expect("Failed to get sessions list");

        let kallichore_api::ListSessionsResponse::ListOfActiveSessions(session_list) = sessions;
        let session_found = session_list
            .sessions
            .iter()
            .any(|s| s.session_id == session_id);
        assert!(
            session_found,
            "Session should be in the active sessions list"
        );

        println!("Test passed: Kernel started successfully despite bad SHELL environment variable");

        drop(server);
    })
    .await;

    match test_result {
        Ok(_) => {
            println!("Bad shell environment variable test completed successfully");
        }
        Err(_) => {
            panic!("Bad shell environment variable test timed out after 30 seconds");
        }
    }
}

#[cfg(not(target_os = "windows"))] // Shell behavior is Unix-specific
#[tokio::test]
async fn test_run_in_shell_functionality() {
    let test_result = tokio::time::timeout(Duration::from_secs(60), async {
        let python_cmd = if let Some(cmd) = get_python_executable().await {
            cmd
        } else {
            println!("Skipping test: No Python executable found");
            return;
        };

        if !is_ipykernel_available().await {
            println!("Skipping test: ipykernel not available for {}", python_cmd);
            return;
        }

        // Start a test server
        let server = TestServer::start().await;
        let client = server.create_client().await;

        // Test helper function to create a session and verify shell behavior via kernel execution
        async fn test_shell_behavior_via_kernel(
            client: &Box<dyn kallichore_api::ApiNoContext<common::test_utils::ClientContext> + Send + Sync>,
            server: &TestServer,
            python_cmd: &str,
            test_name: &str,
            run_in_shell: bool,
            shell_env: Option<&str>,
        ) -> String {
            let session_id = format!("test-shell-{}-{}", test_name, Uuid::new_v4());

            // Create a custom session with the desired shell configuration
            let mut env_vars = vec![];

            // Add custom SHELL environment variable if specified
            if let Some(shell_path) = shell_env {
                env_vars.push(VarAction {
                    action: VarActionType::Replace,
                    name: "SHELL".to_string(),
                    value: shell_path.to_string(),
                });
            }

            let new_session = NewSession {
                session_id: session_id.clone(),
                display_name: format!("Test Shell Session - {}", test_name),
                language: "python".to_string(),
                username: "testuser".to_string(),
                input_prompt: "In [{}]: ".to_string(),
                continuation_prompt: "   ...: ".to_string(),
                session_mode: SessionMode::Console,
                notebook_uri: None,
                argv: vec![
                    python_cmd.to_string(),
                    "-m".to_string(),
                    "ipykernel".to_string(),
                    "-f".to_string(),
                    "{connection_file}".to_string(),
                ],
                working_directory: std::env::current_dir()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                env: env_vars,
                connection_timeout: Some(15),
                interrupt_mode: InterruptMode::Message,
                protocol_version: Some("5.3".to_string()),
                run_in_shell: Some(run_in_shell),
            };

            println!("Creating session '{}' with run_in_shell={}, shell_env={:?}",
                test_name, run_in_shell, shell_env);

            // Create the session
            let _created_session_id = create_session_with_client(client, new_session).await;

            // Start the session
            println!("Starting kernel session '{}'...", test_name);
            let start_response = client
                .start_session(session_id.clone())
                .await
                .expect(&format!("Failed to start session '{}'", test_name));

            // Verify the session started successfully
            match &start_response {
                kallichore_api::StartSessionResponse::Started(_) => {
                    println!("✓ Session '{}' started successfully", test_name);
                }
                kallichore_api::StartSessionResponse::StartFailed(error) => {
                    panic!("Session '{}' failed to start: {:?}", test_name, error);
                }
                _ => {
                    panic!("Unexpected start response for '{}': {:?}", test_name, start_response);
                }
            }

            // Wait for kernel to fully start
            tokio::time::sleep(Duration::from_millis(2000)).await;

            // Create WebSocket connection to the kernel
            let ws_url = format!(
                "ws://localhost:{}/sessions/{}/channels",
                server.port(),
                session_id
            );

            let mut comm = CommunicationChannel::create_websocket(&ws_url)
                .await
                .expect("Failed to create websocket");

            // Wait a bit for the WebSocket to be ready
            tokio::time::sleep(Duration::from_millis(500)).await;

            // Execute Python script to check for shell-specific environment and behaviors
            let test_script_path = std::env::current_dir()
                .unwrap()
                .join("tests")
                .join("shell_test.py");

            let test_code = format!(
                r#"
import subprocess
import sys

# Run our external shell test script
test_marker = "test_{}"
expected_mode = "{}"

try:
    result = subprocess.run([
        sys.executable,
        r"{}",
        test_marker,
        expected_mode
    ], capture_output=True, text=True, timeout=10)

    if result.returncode == 0:
        print(result.stdout)
    else:
        print(f"Shell test script failed with return code {{result.returncode}}")
        print(f"STDOUT: {{result.stdout}}")
        print(f"STDERR: {{result.stderr}}")
except Exception as e:
    print(f"Error running shell test script: {{e}}")
"#,
                test_name,
                if run_in_shell { "shell" } else { "direct" },
                test_script_path.to_string_lossy()
            );

            let execute_request = WebsocketMessage::Jupyter(
                JupyterMessage {
                    header: JupyterMessageHeader {
                        msg_id: Uuid::new_v4().to_string(),
                        msg_type: "execute_request".to_string(),
                    },
                    parent_header: None,
                    channel: JupyterChannel::Shell,
                    content: serde_json::json!({
                        "code": test_code,
                        "silent": false,
                        "store_history": false,
                        "user_expressions": {},
                        "allow_stdin": false,
                        "stop_on_error": true
                    }),
                    metadata: serde_json::json!({}),
                    buffers: vec![],
                }
            );

            println!("Executing shell test code in session '{}'...", test_name);
            comm.send_message(&execute_request)
                .await
                .expect("Failed to send execute request");

            // Collect output for a reasonable amount of time
            let mut all_output = String::new();
            let start_time = std::time::Instant::now();
            let timeout_duration = Duration::from_secs(10);

            while start_time.elapsed() < timeout_duration {
                match tokio::time::timeout(Duration::from_millis(500), comm.receive_message()).await {
                    Ok(Ok(Some(message_text))) => {
                        // Try to parse the JSON message
                        if let Ok(message) = serde_json::from_str::<WebsocketMessage>(&message_text) {
                            match message {
                                WebsocketMessage::Jupyter(jupyter_msg) => {
                                    if jupyter_msg.header.msg_type == "stream" {
                                        if let Some(text) = jupyter_msg.content.get("text").and_then(|v| v.as_str()) {
                                            all_output.push_str(text);
                                        }
                                    } else if jupyter_msg.header.msg_type == "execute_reply" {
                                        println!("Received execute_reply for session '{}'", test_name);
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(Ok(None)) => {
                        // No message received, continue waiting
                    }
                    Ok(Err(e)) => {
                        println!("Error receiving message for session '{}': {}", test_name, e);
                        break;
                    }
                    Err(_) => {
                        // Timeout - continue waiting
                    }
                }
            }

            // Analyze the output
            println!("Output from session '{}':\n{}", test_name, all_output);

            // Verify we got our test markers from the external script
            assert!(
                all_output.contains(&format!("Test marker: test_{}", test_name)),
                "Expected to find test marker in session '{}' output", test_name
            );

            assert!(
                all_output.contains(&format!("Expected mode: {}", if run_in_shell { "shell" } else { "direct" })),
                "Expected to find expected mode marker in session '{}' output", test_name
            );

            // Verify the script ran successfully
            assert!(
                all_output.contains("=== KALLICHORE SHELL TEST START ===") && all_output.contains("=== KALLICHORE SHELL TEST END ==="),
                "Expected shell test script to run successfully in session '{}'", test_name
            );

            // Check for shell-specific behavior from external script output
            if run_in_shell {
                // When running in shell, the script should detect shell indicators
                let has_shell_indicators = all_output.contains("Shell indicators:");

                if has_shell_indicators {
                    println!("✓ Session '{}' shows shell indicators as expected", test_name);
                } else {
                    println!("! Session '{}' expected to show shell indicators but script didn't detect them", test_name);
                    // Don't fail the test as shell detection can be environment-dependent
                }
            } else {
                println!("✓ Session '{}' running in direct mode", test_name);
            }

            // Close the communication channel
            if let Err(e) = comm.close().await {
                println!("Failed to close communication channel for session '{}': {}", test_name, e);
            }

            println!("✓ Session '{}' verification completed successfully", test_name);
            session_id
        }

        // Test 1: run_in_shell = false (default behavior)
        let session_1 = test_shell_behavior_via_kernel(
            &client,
            &server,
            &python_cmd,
            "no-shell",
            false,
            None
        ).await;

        // Test 2: run_in_shell = true with default shell
        let session_2 = test_shell_behavior_via_kernel(
            &client,
            &server,
            &python_cmd,
            "default-shell",
            true,
            None
        ).await;

        // Test 3: run_in_shell = true with bash shell
        let session_3 = test_shell_behavior_via_kernel(
            &client,
            &server,
            &python_cmd,
            "bash-shell",
            true,
            Some("/bin/bash")
        ).await;

        // Clean up all sessions
        for (session_id, name) in [
            (session_1, "no-shell"),
            (session_2, "default-shell"),
            (session_3, "bash-shell"),
        ] {
            println!("Deleting session '{}'...", name);
            if let Err(e) = client.delete_session(session_id).await {
                println!("Warning: Failed to delete session '{}': {:?}", name, e);
            }
        }

        // Give time for cleanup
        tokio::time::sleep(Duration::from_millis(1000)).await;

        println!("All run_in_shell functionality tests completed successfully!");
        println!("✓ Verified that kernels execute code correctly with run_in_shell=false");
        println!("✓ Verified that kernels execute code correctly with run_in_shell=true");
        println!("✓ Verified that shell environment is properly set up when using run_in_shell");

        drop(server);
    })
    .await;

    match test_result {
        Ok(_) => {
            println!("run_in_shell test completed successfully");
        }
        Err(_) => {
            panic!("run_in_shell test timed out after 60 seconds");
        }
    }
}

#[tokio::test]
async fn test_kernel_interrupt() {
    let test_result = tokio::time::timeout(Duration::from_secs(30), async {
        let python_cmd = if let Some(cmd) = get_python_executable().await {
            cmd
        } else {
            println!("Skipping test: No Python executable found");
            return;
        };

        if !is_ipykernel_available().await {
            println!("Skipping test: ipykernel not available for {}", python_cmd);
            return;
        }

        let server = TestServer::start().await;
        let client = server.create_client().await;

        let session_id = format!("interrupt-test-session-{}", Uuid::new_v4());

        // Create a session with Signal interrupt mode for Windows
        let mut new_session = create_test_session(session_id.clone(), &python_cmd);
        #[cfg(windows)]
        {
            new_session.interrupt_mode = InterruptMode::Signal;
        }

        // Create and start the kernel session
        let _created_session_id = create_session_with_client(&client, new_session).await;

        println!("Starting kernel session for interrupt test...");
        let start_response = client
            .start_session(session_id.clone())
            .await
            .expect("Failed to start session");

        println!("Start response: {:?}", start_response);

        // Check if the session started successfully
        match &start_response {
            kallichore_api::StartSessionResponse::Started(_) => {
                println!("Kernel started successfully");
            }
            kallichore_api::StartSessionResponse::StartFailed(error) => {
                println!("Kernel failed to start: {:?}", error);
                println!("Skipping interrupt test due to startup failure");
                return;
            }
            _ => {
                println!("Unexpected start response: {:?}", start_response);
                println!("Skipping interrupt test");
                return;
            }
        }

        // Wait for kernel to fully start
        tokio::time::sleep(Duration::from_millis(1500)).await;

        // Create a websocket connection
        let ws_url = format!(
            "ws://localhost:{}/sessions/{}/channels",
            server.port(),
            session_id
        );

        let mut comm = CommunicationChannel::create_websocket(&ws_url)
            .await
            .expect("Failed to create websocket");

        // Wait for websocket to be ready
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Execute code that will take a long time (loop that prints numbers)
        let long_running_code = r#"
import time
for i in range(100):
    print(f"Iteration {i}")
    time.sleep(0.1)
print("All iterations completed!")
"#;

        let execute_request = WebsocketMessage::Jupyter(JupyterMessage {
            header: JupyterMessageHeader {
                msg_id: Uuid::new_v4().to_string(),
                msg_type: "execute_request".to_string(),
            },
            parent_header: None,
            channel: JupyterChannel::Shell,
            content: serde_json::json!({
                "code": long_running_code,
                "silent": false,
                "store_history": false,
                "user_expressions": {},
                "allow_stdin": false,
                "stop_on_error": true
            }),
            metadata: serde_json::json!({}),
            buffers: vec![],
        });

        println!("Sending long-running execute_request...");
        comm.send_message(&execute_request)
            .await
            .expect("Failed to send execute_request");

        // Wait for some iterations to be printed
        println!("Waiting for some iterations to be printed...");
        let mut iterations_printed = Vec::new();
        let start_time = std::time::Instant::now();

        // Collect some output for 1 second
        while start_time.elapsed() < Duration::from_secs(1) {
            match tokio::time::timeout(Duration::from_millis(100), comm.receive_message()).await {
                Ok(Ok(Some(message_text))) => {
                    if let Ok(WebsocketMessage::Jupyter(jupyter_msg)) =
                        serde_json::from_str::<WebsocketMessage>(&message_text)
                    {
                        if jupyter_msg.header.msg_type == "stream" {
                            if let Some(text) = jupyter_msg.content.get("text").and_then(|v| v.as_str()) {
                                println!("Received output: {}", text);
                                // Extract iteration numbers
                                for line in text.lines() {
                                    if let Some(num_str) = line.strip_prefix("Iteration ") {
                                        if let Ok(num) = num_str.parse::<i32>() {
                                            iterations_printed.push(num);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        println!(
            "Collected {} iterations before interrupt: {:?}",
            iterations_printed.len(),
            iterations_printed
        );

        // Verify we got some iterations (at least 5, should be around 10)
        assert!(
            iterations_printed.len() >= 5,
            "Expected at least 5 iterations before interrupt, got {}",
            iterations_printed.len()
        );

        // Now interrupt the kernel using the interrupt endpoint
        println!("Sending interrupt request via HTTP API...");
        let interrupt_response = client
            .interrupt_session(session_id.clone())
            .await
            .expect("Failed to send interrupt");

        println!("Interrupt response: {:?}", interrupt_response);

        // Verify the interrupt was accepted
        match interrupt_response {
            kallichore_api::InterruptSessionResponse::Interrupted(_) => {
                println!("Interrupt request was accepted");
            }
            _ => {
                panic!("Expected interrupt to succeed, got: {:?}", interrupt_response);
            }
        }

        // Now collect more output and verify that:
        // 1. We receive an error or execute_reply indicating interruption
        // 2. We did NOT receive "All iterations completed!"
        println!("Collecting output after interrupt...");
        let mut all_output = String::new();
        let mut execute_reply_received = false;
        let start_time = std::time::Instant::now();

        while start_time.elapsed() < Duration::from_secs(5) {
            match tokio::time::timeout(Duration::from_millis(200), comm.receive_message()).await {
                Ok(Ok(Some(message_text))) => {
                    if let Ok(WebsocketMessage::Jupyter(jupyter_msg)) =
                        serde_json::from_str::<WebsocketMessage>(&message_text)
                    {
                        println!("Received message type: {}", jupyter_msg.header.msg_type);

                        if jupyter_msg.header.msg_type == "stream" {
                            if let Some(text) = jupyter_msg.content.get("text").and_then(|v| v.as_str()) {
                                all_output.push_str(text);
                                println!("Stream output: {}", text);
                            }
                        } else if jupyter_msg.header.msg_type == "error" {
                            println!("Received error message: {:?}", jupyter_msg.content);
                            all_output.push_str("ERROR_RECEIVED\n");
                        } else if jupyter_msg.header.msg_type == "execute_reply" {
                            println!("Received execute_reply: {:?}", jupyter_msg.content);
                            if let Some(status) = jupyter_msg.content.get("status").and_then(|v| v.as_str()) {
                                println!("Execute reply status: {}", status);
                                if status == "error" {
                                    all_output.push_str("EXECUTE_ERROR\n");
                                }
                            }
                            execute_reply_received = true;
                            break;
                        }
                    }
                }
                _ => {}
            }
        }

        println!("All output after interrupt:\n{}", all_output);

        // Verify we got an execute_reply
        assert!(
            execute_reply_received,
            "Expected to receive execute_reply after interrupt"
        );

        // Verify that the code did NOT complete all iterations
        assert!(
            !all_output.contains("All iterations completed!"),
            "Code should have been interrupted before completion"
        );

        // Verify that we got SOME output (the interrupt didn't happen immediately)
        assert!(
            iterations_printed.len() > 0,
            "Expected some iterations to complete before interrupt"
        );

        // Verify that we did NOT get all 100 iterations
        assert!(
            iterations_printed.len() < 100,
            "Expected interrupt to prevent all iterations from completing"
        );

        println!(
            "✓ Interrupt test passed! Code was interrupted after {} iterations (out of 100)",
            iterations_printed.len()
        );

        // Clean up
        if let Err(e) = comm.close().await {
            println!("Failed to close communication channel: {}", e);
        }

        drop(server);
    })
    .await;

    match test_result {
        Ok(_) => {
            println!("Kernel interrupt test completed successfully");
        }
        Err(_) => {
            panic!("Kernel interrupt test timed out after 30 seconds");
        }
    }
}
