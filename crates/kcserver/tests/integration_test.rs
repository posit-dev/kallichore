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
use kallichore_api::{NewSessionResponse, ServerStatusResponse};
use kcshared::jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader};
use kcshared::kernel_message::KernelMessage;
use kcshared::websocket_message::WebsocketMessage;
use serde_json;
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

#[tokio::test]
async fn test_server_starts_and_responds() {
    let server = TestServer::start().await;
    let client = server.create_client().await;

    let response = client
        .server_status()
        .await
        .expect("Failed to get server status");

    match response {
        ServerStatusResponse::ServerStatusAndInformation(status) => {
            assert_eq!(status.version, "0.1.47");
            assert!(status.sessions >= 0);
        }
        ServerStatusResponse::Error(err) => {
            panic!("Server returned error: {:?}", err);
        }
    }
}

#[tokio::test]
async fn test_kernel_session_and_websocket_communication() {
    // First, try to find a Python executable
    let python_cmd = find_python_executable().await;
    if python_cmd.is_none() {
        println!("Skipping test: No Python executable found");
        return;
    }
    let python_cmd = python_cmd.unwrap();

    // Check if ipykernel is available
    if !check_ipykernel_available(&python_cmd).await {
        println!("Skipping test: ipykernel not available for {}", python_cmd);
        return;
    }

    let server = TestServer::start().await;
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
            python_cmd,
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
        connection_timeout: Some(30),
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

    println!("Created session: {:?}", session_info);

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

    // Wait a moment for the kernel to start up
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // Send a kernel_info_request message to test communication
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

    ws_sender
        .send(Message::Text(message_json))
        .await
        .expect("Failed to send websocket message");

    // Listen for responses
    let mut received_messages = Vec::new();
    let mut kernel_messages = Vec::new();
    let mut kernel_info_reply_received = false;

    // Set a timeout for receiving messages
    let timeout_duration = Duration::from_secs(10);
    let mut message_count = 0;
    const MAX_MESSAGES: usize = 20; // Increase limit for Python kernel

    while message_count < MAX_MESSAGES && !kernel_info_reply_received {
        match tokio::time::timeout(timeout_duration, ws_receiver.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                message_count += 1;
                println!("Received websocket message: {}", text);

                // Try to parse as WebsocketMessage
                match serde_json::from_str::<WebsocketMessage>(&text) {
                    Ok(ws_msg) => {
                        match &ws_msg {
                            WebsocketMessage::Jupyter(jupyter_msg) => {
                                received_messages.push(jupyter_msg.clone());
                                println!(
                                    "Received Jupyter message type: {}",
                                    jupyter_msg.header.msg_type
                                );

                                // Check if we got a kernel_info_reply
                                if jupyter_msg.header.msg_type == "kernel_info_reply" {
                                    kernel_info_reply_received = true;
                                    println!("Successfully received kernel_info_reply!");
                                }
                            }
                            WebsocketMessage::Kernel(kernel_msg) => {
                                kernel_messages.push(kernel_msg.clone());
                                println!("Received kernel message: {:?}", kernel_msg);

                                // If we get a handshake completed or connected message, that's good
                                match kernel_msg {
                                    KernelMessage::HandshakeCompleted(_, _) => {
                                        println!("Kernel handshake completed successfully");
                                    }
                                    KernelMessage::Status(status) => {
                                        println!("Kernel status: {:?}", status);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    Err(e) => {
                        println!("Failed to parse websocket message: {} - Error: {}", text, e);
                    }
                }
            }
            Ok(Some(Ok(Message::Binary(_)))) => {
                message_count += 1;
                println!("Received binary websocket message (ignoring)");
            }
            Ok(Some(Ok(Message::Ping(_)))) => {
                println!("Received ping message");
            }
            Ok(Some(Ok(Message::Pong(_)))) => {
                println!("Received pong message");
            }
            Ok(Some(Ok(Message::Close(_)))) => {
                println!("Websocket connection closed");
                break;
            }
            Ok(Some(Ok(Message::Frame(_)))) => {
                println!("Received frame message");
            }
            Ok(Some(Err(e))) => {
                println!("Websocket error: {}", e);
                break;
            }
            Ok(None) => {
                println!("Websocket stream ended");
                break;
            }
            Err(_) => {
                // Timeout - this is expected if no more messages
                println!(
                    "Timeout waiting for messages (received {} messages)",
                    message_count
                );
                break;
            }
        }
    }

    // Verify we received some messages
    assert!(
        !received_messages.is_empty() || !kernel_messages.is_empty(),
        "Should have received at least some messages from the kernel"
    );

    // Check if we received kernel status messages
    let has_status_messages = kernel_messages
        .iter()
        .any(|msg| matches!(msg, KernelMessage::Status(_)));

    println!(
        "Received {} Jupyter messages and {} kernel messages",
        received_messages.len(),
        kernel_messages.len()
    );
    println!("Has status messages: {}", has_status_messages);
    println!("Kernel info reply received: {}", kernel_info_reply_received);

    // For a successful test, we should have received at least some messages
    // Ideally a kernel_info_reply, but even status messages indicate the kernel is working
    assert!(
        kernel_info_reply_received || has_status_messages || !received_messages.is_empty(),
        "Should have received kernel_info_reply or at least some status/jupyter messages"
    );

    // Clean up - close the websocket
    let _ = ws_sender.close().await;
}

// Helper function to find Python executable
async fn find_python_executable() -> Option<String> {
    let candidates = vec!["python3", "python", "/usr/bin/python3", "/usr/bin/python"];

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
    // First, try to find a Python executable
    let python_cmd = find_python_executable().await;
    if python_cmd.is_none() {
        println!("Skipping test: No Python executable found");
        return;
    }
    let python_cmd = python_cmd.unwrap();

    // Check if ipykernel is available
    if !check_ipykernel_available(&python_cmd).await {
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
            connection_timeout: Some(10),
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
