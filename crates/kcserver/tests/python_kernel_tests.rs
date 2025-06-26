//
// python_kernel_tests.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//

//! Python kernel communication tests

#[path = "common/mod.rs"]
mod common;

use common::test_utils::{
    create_execute_request, create_session_with_client,
    create_test_session, get_python_executable, is_ipykernel_available,
};
use common::transport::{run_communication_test, CommunicationChannel, TransportType};
use common::TestServer;
use kallichore_api::models::{InterruptMode, NewSession};
use std::time::Duration;
use uuid::Uuid;

/// Run a Python kernel test with the specified transport
async fn run_python_kernel_test_transport(python_cmd: &str, transport: TransportType) {
    // For domain socket transport, we need to start a Unix socket server
    #[cfg(unix)]
    if matches!(transport, TransportType::DomainSocket) {
        return run_python_kernel_test_domain_socket(python_cmd).await;
    }

    let server = TestServer::start().await;
    let client = server.create_client().await;

    // Create a kernel session using Python with ipykernel
    let session_id = format!("test-session-{}", Uuid::new_v4());
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
            CommunicationChannel::create_domain_socket(&socket_path)
                .await
                .expect("Failed to create domain socket")
        }
        #[cfg(windows)]
        TransportType::NamedPipe => {
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
            CommunicationChannel::create_named_pipe(&pipe_path)
                .await
                .expect("Failed to create named pipe")
        }
    };

    // Wait for the kernel to start
    println!("Waiting for Python kernel to start up...");
    tokio::time::sleep(Duration::from_millis(800)).await; // Give kernel time to start

    // Send an execute_request directly (kernel_info already happens during startup)
    let execute_request = create_execute_request();
    println!("Sending execute_request to Python kernel...");
    comm.send_message(&execute_request)
        .await
        .expect("Failed to send execute_request");

    // Run the communication test with reasonable timeout to get all results
    let timeout = Duration::from_secs(12);
    let max_messages = 25;
    let results = run_communication_test(&mut comm, timeout, max_messages).await;

    results.print_summary();
    
    // Assert only the essential functionality for faster tests
    assert!(
        results.execute_reply_received,
        "Expected to receive execute_reply from Python kernel, but didn't get one. The kernel is not executing code properly."
    );

    assert!(
        results.stream_output_received,
        "Expected to receive stream output from Python kernel, but didn't get any. The kernel is not producing stdout output."
    );

    assert!(
        results.expected_output_found,
        "Expected to find 'Hello from Kallichore test!' and '2 + 3 = 5' in the kernel output, but didn't find both. The kernel executed but produced unexpected output. Actual collected output: {:?}",
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

            run_python_kernel_test_transport(&python_cmd, TransportType::Websocket).await;
        },
    )
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
        r#"{{"session_id": "{}", "display_name": "Test Session", "language": "python", "username": "testuser", "input_prompt": "In [{{}}]: ", "continuation_prompt": "   ...: ", "argv": ["{}", "-m", "ipykernel", "-f", "{{connection_file}}"], "working_directory": "/tmp", "env": [], "connection_timeout": 60, "interrupt_mode": "message", "protocol_version": "5.3", "run_in_shell": false}}"#,
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

    // Send an execute_request directly (kernel_info already happens during startup)
    let execute_request = create_execute_request();
    println!("Sending execute_request to Python kernel...");
    comm.send_message(&execute_request)
        .await
        .expect("Failed to send execute_request");

    // Run the communication test with reasonable timeout to get all results
    let timeout = Duration::from_secs(12);
    let max_messages = 25;
    let results = run_communication_test(&mut comm, timeout, max_messages).await;

    results.print_summary();
    
    // Assert only the essential functionality for faster domain socket tests
    assert!(
        results.execute_reply_received,
        "Expected to receive execute_reply from Python kernel, but didn't get one. The kernel is not executing code properly."
    );

    assert!(
        results.stream_output_received,
        "Expected to receive stream output from Python kernel, but didn't get any. The kernel is not producing stdout output."
    );

    assert!(
        results.expected_output_found,
        "Expected to find 'Hello from Kallichore test!' and '2 + 3 = 5' in the kernel output, but didn't find both. The kernel executed but produced unexpected output. Actual collected output: {:?}",
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
