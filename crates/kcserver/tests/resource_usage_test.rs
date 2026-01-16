//
// resource_usage_test.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Tests for kernel resource usage monitoring

#![allow(unused_imports)]

#[path = "common/mod.rs"]
mod common;

use common::test_utils::{
    create_session_with_client, create_test_session, get_python_executable,
    is_ipykernel_available,
};
use common::transport::CommunicationChannel;
use common::TestServer;
use kcshared::kernel_message::KernelMessage;
use kcshared::websocket_message::WebsocketMessage;
use std::time::Duration;
use uuid::Uuid;

/// Test that resource usage is populated in the session info via the HTTP API
/// and that we receive resource usage updates over WebSocket.
#[tokio::test]
async fn test_resource_usage_populated() {
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

        let session_id = format!("resource-usage-test-{}", Uuid::new_v4());
        let new_session = create_test_session(session_id.clone(), &python_cmd);

        // Create the kernel session
        let _created_session_id = create_session_with_client(&client, new_session).await;

        println!("Starting kernel session for resource usage test...");
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
                println!("Skipping resource usage test due to startup failure");
                return;
            }
            _ => {
                println!("Unexpected start response: {:?}", start_response);
                println!("Skipping resource usage test");
                return;
            }
        }

        // Wait for kernel to fully start
        tokio::time::sleep(Duration::from_millis(1500)).await;

        // Create a WebSocket connection to trigger resource monitoring
        let ws_url = format!(
            "ws://localhost:{}/sessions/{}/channels",
            server.port(),
            session_id
        );

        let mut comm = CommunicationChannel::create_websocket(&ws_url)
            .await
            .expect("Failed to create websocket");

        // Wait for resource usage to be sampled (default interval is 1000ms)
        // We need to wait for at least 2 sampling periods to ensure we get data
        println!("Waiting for resource usage sampling...");
        tokio::time::sleep(Duration::from_millis(2500)).await;

        // Check that resource_usage is populated in the session info
        let sessions = client
            .list_sessions()
            .await
            .expect("Failed to list sessions");

        let kallichore_api::ListSessionsResponse::ListOfActiveSessions(session_list) = sessions;
        let session = session_list
            .sessions
            .iter()
            .find(|s| s.session_id == session_id)
            .expect("Session should be in the list");

        println!("Session resource_usage: {:?}", session.resource_usage);

        // Verify resource_usage is populated
        let resource_usage = session
            .resource_usage
            .as_ref()
            .expect("resource_usage should be populated");

        // Verify that we have nonzero values for memory (a running Python process
        // will always use some memory)
        assert!(
            resource_usage.memory_bytes > 0,
            "Expected memory_bytes to be > 0, got {}",
            resource_usage.memory_bytes
        );

        // Thread count should be at least 1
        assert!(
            resource_usage.thread_count >= 1,
            "Expected thread_count >= 1, got {}",
            resource_usage.thread_count
        );

        // Sampling period should match the default (1000ms)
        assert!(
            resource_usage.sampling_period_ms > 0,
            "Expected sampling_period_ms > 0, got {}",
            resource_usage.sampling_period_ms
        );

        // Timestamp should be set
        assert!(
            resource_usage.timestamp > 0,
            "Expected timestamp > 0, got {}",
            resource_usage.timestamp
        );

        println!(
            "Resource usage validated: memory={}B, threads={}, cpu={}%, period={}ms",
            resource_usage.memory_bytes,
            resource_usage.thread_count,
            resource_usage.cpu_percent,
            resource_usage.sampling_period_ms
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
            println!("Resource usage test completed successfully");
        }
        Err(_) => {
            panic!("Resource usage test timed out after 30 seconds");
        }
    }
}

/// Test that resource usage updates are received over WebSocket
#[tokio::test]
async fn test_resource_usage_websocket_messages() {
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

        let session_id = format!("resource-ws-test-{}", Uuid::new_v4());
        let new_session = create_test_session(session_id.clone(), &python_cmd);

        // Create the kernel session
        let _created_session_id = create_session_with_client(&client, new_session).await;

        println!("Starting kernel session for WebSocket resource usage test...");
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
                println!("Skipping WebSocket resource usage test due to startup failure");
                return;
            }
            _ => {
                println!("Unexpected start response: {:?}", start_response);
                println!("Skipping WebSocket resource usage test");
                return;
            }
        }

        // Wait for kernel to start
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Create a WebSocket connection
        let ws_url = format!(
            "ws://localhost:{}/sessions/{}/channels",
            server.port(),
            session_id
        );

        let mut comm = CommunicationChannel::create_websocket(&ws_url)
            .await
            .expect("Failed to create websocket");

        // Listen for resource usage messages over WebSocket
        // Default sampling interval is 1000ms, so we should receive at least one
        // within 5 seconds
        println!("Listening for resource usage WebSocket messages...");
        let mut resource_update_received = false;
        let mut received_update: Option<kcshared::kernel_message::ResourceUpdate> = None;
        let start_time = std::time::Instant::now();

        while start_time.elapsed() < Duration::from_secs(5) {
            match tokio::time::timeout(Duration::from_millis(500), comm.receive_message()).await {
                Ok(Ok(Some(message_text))) => {
                    // Try to parse as WebsocketMessage
                    if let Ok(ws_msg) = serde_json::from_str::<WebsocketMessage>(&message_text) {
                        if let WebsocketMessage::Kernel(KernelMessage::ResourceUsage(update)) =
                            ws_msg
                        {
                            println!(
                                "Received resource usage update: memory={}B, threads={}, cpu={}%",
                                update.memory_bytes, update.thread_count, update.cpu_percent
                            );
                            resource_update_received = true;
                            received_update = Some(update);
                            break;
                        }
                    }
                }
                Ok(Ok(None)) => {
                    // No message, continue waiting
                }
                Ok(Err(e)) => {
                    println!("Error receiving message: {}", e);
                    break;
                }
                Err(_) => {
                    // Timeout on this iteration, continue waiting
                }
            }
        }

        assert!(
            resource_update_received,
            "Expected to receive a resource usage update over WebSocket within 5 seconds"
        );

        let update = received_update.expect("Should have received an update");

        // Verify the update has valid data
        assert!(
            update.memory_bytes > 0,
            "Expected memory_bytes > 0, got {}",
            update.memory_bytes
        );

        assert!(
            update.thread_count >= 1,
            "Expected thread_count >= 1, got {}",
            update.thread_count
        );

        assert!(
            update.sampling_period_ms > 0,
            "Expected sampling_period_ms > 0, got {}",
            update.sampling_period_ms
        );

        assert!(
            update.timestamp > 0,
            "Expected timestamp > 0, got {}",
            update.timestamp
        );

        println!("WebSocket resource usage test passed!");

        // Clean up
        if let Err(e) = comm.close().await {
            println!("Failed to close communication channel: {}", e);
        }

        drop(server);
    })
    .await;

    match test_result {
        Ok(_) => {
            println!("WebSocket resource usage test completed successfully");
        }
        Err(_) => {
            panic!("WebSocket resource usage test timed out after 30 seconds");
        }
    }
}

/// Verify that multiple concurrent kernel sessions get accurate CPU usage measurements.
#[tokio::test]
async fn test_multi_session_cpu_tracking() {
    let test_result = tokio::time::timeout(Duration::from_secs(45), async {
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

        // Create two kernel sessions
        let session_id_1 = format!("multi-session-1-{}", Uuid::new_v4());
        let session_id_2 = format!("multi-session-2-{}", Uuid::new_v4());

        let new_session_1 = create_test_session(session_id_1.clone(), &python_cmd);
        let new_session_2 = create_test_session(session_id_2.clone(), &python_cmd);

        println!("Creating two kernel sessions...");
        let _created_1 = create_session_with_client(&client, new_session_1).await;
        let _created_2 = create_session_with_client(&client, new_session_2).await;

        // Start both sessions
        println!("Starting session 1...");
        let start_response_1 = client
            .start_session(session_id_1.clone())
            .await
            .expect("Failed to start session 1");

        match &start_response_1 {
            kallichore_api::StartSessionResponse::Started(_) => {
                println!("Session 1 started successfully");
            }
            _ => {
                println!("Session 1 failed to start, skipping test");
                return;
            }
        }

        println!("Starting session 2...");
        let start_response_2 = client
            .start_session(session_id_2.clone())
            .await
            .expect("Failed to start session 2");

        match &start_response_2 {
            kallichore_api::StartSessionResponse::Started(_) => {
                println!("Session 2 started successfully");
            }
            _ => {
                println!("Session 2 failed to start, skipping test");
                return;
            }
        }

        // Wait for both kernels to fully start
        tokio::time::sleep(Duration::from_millis(2000)).await;

        // Create WebSocket connections for both sessions to trigger resource monitoring
        let ws_url_1 = format!(
            "ws://localhost:{}/sessions/{}/channels",
            server.port(),
            session_id_1
        );
        let ws_url_2 = format!(
            "ws://localhost:{}/sessions/{}/channels",
            server.port(),
            session_id_2
        );

        let mut comm_1 = CommunicationChannel::create_websocket(&ws_url_1)
            .await
            .expect("Failed to create websocket for session 1");

        let mut comm_2 = CommunicationChannel::create_websocket(&ws_url_2)
            .await
            .expect("Failed to create websocket for session 2");

        // Wait for several sampling periods to collect resource usage data
        println!("Waiting for resource usage sampling across both sessions...");
        tokio::time::sleep(Duration::from_millis(3500)).await;

        // Collect resource usage updates from both sessions
        let mut session_1_updates = Vec::new();
        let mut session_2_updates = Vec::new();

        println!("Collecting resource usage updates...");
        let collection_start = std::time::Instant::now();

        while collection_start.elapsed() < Duration::from_secs(6) {
            // Check session 1
            match tokio::time::timeout(Duration::from_millis(100), comm_1.receive_message()).await {
                Ok(Ok(Some(message_text))) => {
                    if let Ok(ws_msg) = serde_json::from_str::<WebsocketMessage>(&message_text) {
                        if let WebsocketMessage::Kernel(KernelMessage::ResourceUsage(update)) =
                            ws_msg
                        {
                            println!(
                                "Session 1 update: cpu={}%, memory={}B",
                                update.cpu_percent, update.memory_bytes
                            );
                            session_1_updates.push(update);
                        }
                    }
                }
                _ => {}
            }

            // Check session 2
            match tokio::time::timeout(Duration::from_millis(100), comm_2.receive_message()).await {
                Ok(Ok(Some(message_text))) => {
                    if let Ok(ws_msg) = serde_json::from_str::<WebsocketMessage>(&message_text) {
                        if let WebsocketMessage::Kernel(KernelMessage::ResourceUsage(update)) =
                            ws_msg
                        {
                            println!(
                                "Session 2 update: cpu={}%, memory={}B",
                                update.cpu_percent, update.memory_bytes
                            );
                            session_2_updates.push(update);
                        }
                    }
                }
                _ => {}
            }

            // Break if we have enough updates
            if session_1_updates.len() >= 3 && session_2_updates.len() >= 3 {
                break;
            }

            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        println!(
            "Collected {} updates from session 1, {} from session 2",
            session_1_updates.len(),
            session_2_updates.len()
        );

        // Verify we got updates from both sessions
        assert!(
            !session_1_updates.is_empty(),
            "Should have received resource updates from session 1"
        );
        assert!(
            !session_2_updates.is_empty(),
            "Should have received resource updates from session 2"
        );

        // Calculate average CPU percentages for both sessions
        let avg_cpu_1: f64 = session_1_updates
            .iter()
            .map(|u| u.cpu_percent as f64)
            .sum::<f64>()
            / session_1_updates.len() as f64;

        let avg_cpu_2: f64 = session_2_updates
            .iter()
            .map(|u| u.cpu_percent as f64)
            .sum::<f64>()
            / session_2_updates.len() as f64;

        println!(
            "Average CPU usage: Session 1 = {:.1}%, Session 2 = {:.1}%",
            avg_cpu_1, avg_cpu_2
        );

        // Key regression test: verify that session 2 doesn't have artificially inflated CPU
        // Both sessions are idle Python kernels, so they should have similar (low) CPU usage
        // The bug would cause session 2 to show spikes of hundreds of percent
        //
        // We allow a reasonable range: idle kernels might use 0-20% CPU occasionally,
        // but sustained values over 100% indicate the bug has returned
        let max_cpu_2 = session_2_updates
            .iter()
            .map(|u| u.cpu_percent)
            .max()
            .unwrap_or(0);

        assert!(
            max_cpu_2 < 150,
            "Session 2 showed abnormally high CPU usage ({}%), \
             suggesting the multi-session CPU tracking bug has returned. \
             Expected idle kernel to use <150% CPU.",
            max_cpu_2
        );

        // Additional sanity check: both sessions should have comparable average CPU usage
        // (within a factor of 10, since they're both idle)
        let ratio = if avg_cpu_1 > 1.0 && avg_cpu_2 > 1.0 {
            (avg_cpu_1 / avg_cpu_2).max(avg_cpu_2 / avg_cpu_1)
        } else {
            1.0
        };

        assert!(
            ratio < 10.0,
            "Session CPU usage ratio too large ({:.1}:1), suggesting measurement issue. \
             Session 1 avg: {:.1}%, Session 2 avg: {:.1}%",
            ratio,
            avg_cpu_1,
            avg_cpu_2
        );

        println!("Multi-session CPU tracking test passed!");
        println!(
            "Both sessions showed reasonable CPU usage without artificial spikes."
        );

        // Clean up
        let _ = comm_1.close().await;
        let _ = comm_2.close().await;
        drop(server);
    })
    .await;

    match test_result {
        Ok(_) => {
            println!("Multi-session CPU tracking test completed successfully");
        }
        Err(_) => {
            panic!("Multi-session CPU tracking test timed out after 45 seconds");
        }
    }
}
