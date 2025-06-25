//
// python_kernel_tests.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
//

//! Python kernel communication tests

#[path = "common/mod.rs"]
mod common;

use common::test_utils::{
    create_execute_request, create_kernel_info_request, create_session_with_client,
    create_test_session, get_python_executable, is_ipykernel_available,
};
use common::transport::{run_communication_test, CommunicationChannel, TransportType};
use common::TestServer;
use kallichore_api::models::{InterruptMode, NewSession};
use std::time::Duration;
use uuid::Uuid;

/// Run a Python kernel test with the specified transport
async fn run_python_kernel_test_transport(python_cmd: &str, transport: TransportType) {
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
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send a kernel_info_request
    let kernel_info_request = create_kernel_info_request();
    println!("Sending kernel_info_request to Python kernel...");
    comm.send_message(&kernel_info_request)
        .await
        .expect("Failed to send kernel_info_request");

    // Wait for response
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send an execute_request
    let execute_request = create_execute_request();
    println!("Sending execute_request to Python kernel...");
    comm.send_message(&execute_request)
        .await
        .expect("Failed to send execute_request");

    // Run the communication test
    let timeout = Duration::from_secs(15);
    let max_messages = 30;
    let results = run_communication_test(&mut comm, timeout, max_messages).await;

    results.print_summary();
    results.assert_success();

    // Clean up
    if let Err(e) = comm.close().await {
        println!("Failed to close communication channel: {}", e);
    }

    drop(server);
}

#[tokio::test]
async fn test_python_kernel_session_and_websocket_communication() {
    let test_result = tokio::time::timeout(
        Duration::from_secs(25),
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
        Duration::from_secs(25),
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
        Duration::from_secs(25),
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
