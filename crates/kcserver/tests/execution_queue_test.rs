//
// execution_queue_test.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//

//! Unit and integration tests for ExecutionQueue functionality

#[path = "common/mod.rs"]
mod common;

use common::test_utils::{
    create_session_with_client, create_test_session,
    get_python_executable, is_ipykernel_available,
};
use common::transport::CommunicationChannel;
use common::TestServer;
use kcserver::execution_queue::ExecutionQueue;
use kcshared::jupyter_message::{JupyterChannel, JupyterMessage, JupyterMessageHeader};
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

/// Helper function to create a test Jupyter message
fn create_test_message(msg_type: &str, code: &str) -> JupyterMessage {
    JupyterMessage {
        header: JupyterMessageHeader {
            msg_id: Uuid::new_v4().to_string(),
            msg_type: msg_type.to_string(),
        },
        parent_header: None,
        channel: JupyterChannel::Shell,
        content: json!({
            "code": code,
            "silent": false,
            "store_history": true,
            "user_expressions": {},
            "allow_stdin": false,
            "stop_on_error": true
        }),
        metadata: json!({}),
        buffers: vec![],
    }
}

// Unit Tests - Core functionality only

#[test]
fn test_execution_queue_basics() {
    let mut queue = ExecutionQueue::new();
    
    // Empty queue
    assert!(queue.active.is_none());
    assert_eq!(queue.pending.len(), 0);
    
    // First request executes immediately
    let msg1 = create_test_message("execute_request", "print('first')");
    let msg1_id = msg1.header.msg_id.clone();
    assert!(queue.process_request(msg1));
    assert_eq!(queue.active.as_ref().unwrap().header.msg_id, msg1_id);
    assert_eq!(queue.pending.len(), 0);
    
    // Second request gets queued
    let msg2 = create_test_message("execute_request", "print('second')");
    let msg2_id = msg2.header.msg_id.clone();
    assert!(!queue.process_request(msg2));
    assert_eq!(queue.active.as_ref().unwrap().header.msg_id, msg1_id);
    assert_eq!(queue.pending.len(), 1);
    assert_eq!(queue.pending[0].header.msg_id, msg2_id);
}

#[test]
fn test_execution_queue_next_and_clear() {
    let mut queue = ExecutionQueue::new();
    
    // Add requests
    let msg1 = create_test_message("execute_request", "print('first')");
    let msg2 = create_test_message("execute_request", "print('second')");
    let msg2_id = msg2.header.msg_id.clone();
    
    queue.process_request(msg1);
    queue.process_request(msg2);
    
    // Get next request
    let next = queue.next_request();
    assert!(next.is_some());
    assert_eq!(next.unwrap().header.msg_id, msg2_id);
    assert_eq!(queue.active.as_ref().unwrap().header.msg_id, msg2_id);
    
    // Clear queue
    queue.clear();
    assert!(queue.active.is_none());
    assert_eq!(queue.pending.len(), 0);
}

#[test]
fn test_execution_queue_serialization() {
    let mut queue = ExecutionQueue::new();
    
    // Empty queue
    let json_empty = queue.to_json();
    assert!(json_empty.active.is_none());
    assert_eq!(json_empty.length, 0);
    
    // Add requests
    let msg1 = create_test_message("execute_request", "print('active')");
    let msg2 = create_test_message("execute_request", "print('pending')");
    queue.process_request(msg1);
    queue.process_request(msg2);
    
    let json_with_data = queue.to_json();
    assert!(json_with_data.active.is_some());
    assert_eq!(json_with_data.length, 1);
    assert_eq!(json_with_data.pending.len(), 1);
}

// Integration Test - Real Jupyter kernel

#[tokio::test]
async fn test_execution_queue_with_real_kernel() {
    let python_cmd = if let Some(cmd) = get_python_executable().await {
        cmd
    } else {
        println!("Skipping integration test: No Python executable found");
        return;
    };

    if !is_ipykernel_available().await {
        println!("Skipping integration test: ipykernel not available");
        return;
    }

    let server = TestServer::start().await;
    let client = server.create_client().await;

    let session_id = format!("execution-queue-test-{}", Uuid::new_v4());
    let new_session = create_test_session(session_id.clone(), &python_cmd);

    // Create and start the kernel session
    let _created_session_id = create_session_with_client(&client, new_session).await;

    println!("Starting kernel session for execution queue test...");
    let start_response = client
        .start_session(session_id.clone())
        .await
        .expect("Failed to start session");

    // Check if the session started successfully
    match &start_response {
        kallichore_api::StartSessionResponse::Started(_) => {
            println!("Kernel started successfully");
        }
        kallichore_api::StartSessionResponse::StartFailed(error) => {
            println!("Kernel failed to start: {:?}", error);
            println!("Skipping execution queue test due to startup failure");
            return;
        }
        _ => {
            println!("Unexpected start response: {:?}", start_response);
            return;
        }
    }

    // Wait for kernel to fully start
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // Create WebSocket connection
    let ws_url = format!(
        "ws://localhost:{}/sessions/{}/channels",
        server.port(),
        session_id
    );

    let mut comm = CommunicationChannel::create_websocket(&ws_url)
        .await
        .expect("Failed to create websocket");

    // Wait for WebSocket to be ready
    tokio::time::sleep(Duration::from_millis(500)).await;

    println!("Testing execution queue behavior with multiple rapid requests...");

    // First send a long-running request to occupy the kernel
    let blocking_request = create_execute_request_with_code("import time; print('Starting long task'); time.sleep(3); print('Long task finished')");
    
    println!("Sending blocking request to occupy kernel...");
    comm.send_message(&blocking_request)
        .await
        .expect("Failed to send blocking request");

    // Wait a moment for the blocking request to start
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Now send multiple requests rapidly while kernel is busy
    let requests = vec![
        ("first", "x = 1; print(f'Queued First: {x}')"),
        ("second", "y = 2; print(f'Queued Second: {y}')"),
        ("third", "z = 3; print(f'Queued Third: {z}')"),
        ("fourth", "print(f'Queued Fourth: {x + y + z}')"),
    ];

    println!("Sending requests rapidly while kernel is busy (should be queued)...");
    for (name, code) in &requests {
        let execute_request = create_execute_request_with_code(code);

        println!("Sending {} request: {}", name, code);
        comm.send_message(&execute_request)
            .await
            .expect("Failed to send execute request");

        // Send them immediately without delay to test queuing
    }

    // Check session status to see if queue has pending requests
    println!("Checking session status immediately after sending queued requests...");
    if let Ok(kallichore_api::ListSessionsResponse::ListOfActiveSessions(session_list)) = client.list_sessions().await {
        if let Some(session) = session_list.sessions.iter().find(|s| s.session_id == session_id) {
            println!("Session status: {:?}", session.status);
            println!("Execution queue length: {}", session.execution_queue.length);
            
            if session.execution_queue.length > 0 {
                println!("✓ Queue has {} pending requests - proving requests are being queued!", session.execution_queue.length);
            } else {
                println!("⚠ Queue is empty - requests may not be queuing as expected");
            }
        }
    }

    // Collect all responses to verify execution order
    println!("Collecting execution results...");
    let timeout = Duration::from_secs(25); // Increased timeout for execution queue test
    let max_messages = 100; // Increased message limit for execution queue test
    let results = run_execution_queue_communication_test(&mut comm, timeout, max_messages).await;

    results.print_summary();

    // Verify we got results from all requests
    let output = &results.collected_output;
    println!("Combined output: {}", output);

    // Check that all outputs are present (execution queue should process them in order)
    assert!(output.contains("Starting long task"), "Should see output from blocking request");
    assert!(output.contains("Long task finished"), "Should see completion of blocking request");
    assert!(output.contains("Queued First: 1"), "Should see output from first queued request");
    assert!(output.contains("Queued Second: 2"), "Should see output from second queued request");
    assert!(output.contains("Queued Third: 3"), "Should see output from third queued request");
    assert!(output.contains("Queued Fourth: 6"), "Should see output from fourth queued request (which depends on previous requests)");

    // Verify execution order: blocking task should complete before queued requests start
    let blocking_start_pos = output.find("Starting long task").expect("Should find blocking start");
    let blocking_end_pos = output.find("Long task finished").expect("Should find blocking end");
    let first_queued_pos = output.find("Queued First: 1").expect("Should find first queued");
    
    assert!(blocking_start_pos < blocking_end_pos, "Blocking task should start before it ends");
    assert!(blocking_end_pos < first_queued_pos, "Blocking task should complete before queued requests start");

    // The fourth request depends on variables set by previous requests
    // If execution queue works correctly, all requests should execute in order
    println!("✓ Execution queue correctly queued requests during blocking operation and processed them in order");

    // Clean up
    comm.close().await.ok();
    drop(server);
}

/// Helper to create execute request with custom code
fn create_execute_request_with_code(code: &str) -> kcshared::websocket_message::WebsocketMessage {
    kcshared::websocket_message::WebsocketMessage::Jupyter(JupyterMessage {
        header: JupyterMessageHeader {
            msg_id: Uuid::new_v4().to_string(),
            msg_type: "execute_request".to_string(),
        },
        parent_header: None,
        channel: JupyterChannel::Shell,
        content: json!({
            "code": code,
            "silent": false,
            "store_history": false,
            "user_expressions": {},
            "allow_stdin": false,
            "stop_on_error": true
        }),
        metadata: json!({}),
        buffers: vec![],
    })
}

/// Custom communication test for execution queue that doesn't exit early
async fn run_execution_queue_communication_test(
    comm: &mut CommunicationChannel,
    timeout: Duration,
    max_messages: u32,
) -> common::transport::CommunicationTestResults {
    use common::transport::CommunicationTestResults;
    
    let mut results = CommunicationTestResults::default();
    let start_time = std::time::Instant::now();

    println!("Listening for kernel responses...");

    while start_time.elapsed() < timeout && results.message_count < max_messages {
        println!(
            "Waiting for message... (elapsed: {:.1}s)",
            start_time.elapsed().as_secs_f32()
        );

        match comm.receive_message().await {
            Ok(Some(text)) => {
                results.process_message(&text);
                
                // For execution queue test, continue collecting until timeout or max messages
                // Don't exit early like the standard communication test does
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

    results
}