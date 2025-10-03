//
// connection_file_tests.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Tests for server startup with connection files

#[path = "common/mod.rs"]
mod common;

use common::test_utils::{cleanup_spawned_server, create_server_command, wait_for_connection_file};
use kallichore_api::{ApiNoContext, Client, ContextWrapperExt, ServerStatusResponse};
use serde_json;
use std::process::Child;
use std::time::Duration;
use swagger::{AuthData, ContextBuilder, EmptyContext, Push, XSpanIdString};
use uuid::Uuid;

type ClientContext = swagger::make_context_ty!(
    ContextBuilder,
    EmptyContext,
    Option<AuthData>,
    XSpanIdString
);

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct ServerConnectionInfo {
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

/// Create a client for the given base path
async fn create_client_for_base_path(
    base_path: &str,
    bearer_token: Option<&str>,
) -> Box<dyn ApiNoContext<ClientContext> + Send + Sync> {
    let context: ClientContext = if let Some(token) = bearer_token {
        swagger::make_context!(
            ContextBuilder,
            EmptyContext,
            Some(AuthData::Bearer(swagger::auth::Bearer {
                token: token.to_string()
            })),
            XSpanIdString::default()
        )
    } else {
        #[allow(trivial_casts)]
        let context: ClientContext = swagger::make_context!(
            ContextBuilder,
            EmptyContext,
            None as Option<AuthData>,
            XSpanIdString::default()
        );
        context
    };

    let client = Client::try_new_http(base_path).expect("Failed to create HTTP client");
    Box::new(client.with_context(context))
}

/// Wait for server to be ready by polling status
async fn wait_for_server_ready(client: &Box<dyn ApiNoContext<ClientContext> + Send + Sync>) {
    let mut ready = false;
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
}

/// Test basic server startup with connection file
async fn test_connection_file_startup(
    extra_args: &[&str],
    test_name: &str,
    expected_transport: &str,
) -> (ServerConnectionInfo, Child) {
    let temp_dir = std::env::temp_dir();
    let connection_file_path = temp_dir.join(format!(
        "kallichore_test_{}_{}.json",
        test_name,
        Uuid::new_v4()
    ));
    let connection_file_str = connection_file_path.to_string_lossy().to_string();

    let mut args = vec![
        "--port",
        "0",
        "--connection-file",
        &connection_file_str,
        "--token",
        "none",
    ];
    args.extend_from_slice(extra_args);

    let mut cmd = create_server_command(&args);
    let mut child = cmd.spawn().expect("Failed to start kcserver");

    // Wait for connection file to be created
    if let Err(e) = wait_for_connection_file(&connection_file_path, 100).await {
        // Try to get error output from the process
        if let Ok(output) = child.try_wait() {
            if let Some(_exit_status) = output {
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
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        panic!("{}", e);
    }

    // Read the connection file
    let connection_content =
        std::fs::read_to_string(&connection_file_path).expect("Failed to read connection file");

    let connection_info: ServerConnectionInfo =
        serde_json::from_str(&connection_content).expect("Failed to parse connection file");

    // Verify the connection info
    assert_eq!(connection_info.transport, expected_transport);
    assert!(connection_info.server_pid > 0, "PID should be greater than 0");

    // Return both connection info and child process (caller will cleanup)
    // Remove connection file but keep socket file for caller to test
    let _ = std::fs::remove_file(&connection_file_path);

    (connection_info, child)
}

#[tokio::test]
async fn test_server_starts_with_connection_file() {
    let (connection_info, child) =
        test_connection_file_startup(&["--transport", "tcp"], "tcp", "tcp").await;

    assert!(connection_info.port.is_some(), "Port should be present");
    assert!(
        connection_info.base_path.is_some(),
        "Base path should be present"
    );
    assert!(
        connection_info.socket_path.is_none(),
        "Socket path should be None"
    );

    let port = connection_info.port.unwrap();
    let base_path = connection_info.base_path.unwrap();

    assert!(port > 0, "Port should be greater than 0");
    assert_eq!(
        base_path,
        format!("http://127.0.0.1:{}", port),
        "Base path format should be correct"
    );
    assert_eq!(
        connection_info.bearer_token, None,
        "Token should be None when disabled"
    );

    println!(
        "Successfully tested server with connection file. Port: {}",
        port
    );

    // Clean up
    cleanup_spawned_server(child);
}

#[tokio::test]
async fn test_server_connection_file_with_auth_token() {
    let temp_dir = std::env::temp_dir();
    let connection_file_path =
        temp_dir.join(format!("kallichore_test_auth_{}.json", Uuid::new_v4()));
    let connection_file_str = connection_file_path.to_string_lossy().to_string();

    // Create a temporary token file
    let token_file_path = temp_dir.join(format!("kallichore_test_token_{}.txt", Uuid::new_v4()));
    let token_file_str = token_file_path.to_string_lossy().to_string();
    let test_token = "test_auth_token_12345";
    std::fs::write(&token_file_path, test_token).expect("Failed to write token file");

    let args = [
        "--port",
        "0",
        "--connection-file",
        &connection_file_str,
        "--transport",
        "tcp",
        "--token",
        &token_file_str,
    ];

    let mut cmd = create_server_command(&args);
    let child = cmd.spawn().expect("Failed to start kcserver");

    // Wait for connection file
    wait_for_connection_file(&connection_file_path, 100)
        .await
        .expect("Connection file was not created");

    // Read the connection file
    let connection_content =
        std::fs::read_to_string(&connection_file_path).expect("Failed to read connection file");

    let connection_info: ServerConnectionInfo =
        serde_json::from_str(&connection_content).expect("Failed to parse connection file");

    // Verify the connection info includes the auth token
    assert!(connection_info.port.is_some());
    assert_eq!(connection_info.bearer_token, Some(test_token.to_string()));

    let base_path = connection_info.base_path.unwrap();

    // Test that we can connect with the proper auth token
    let client_with_auth = create_client_for_base_path(&base_path, Some(test_token)).await;
    let client_no_auth = create_client_for_base_path(&base_path, None).await;

    wait_for_server_ready(&client_with_auth).await;

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

    // Test unauthenticated request for server status (this should work)
    let unauth_result = client_no_auth.server_status().await;
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
        connection_info.port.unwrap()
    );
}

#[tokio::test]
async fn test_multiple_servers_different_ports() {
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

        let args = [
            "--port",
            "0",
            "--connection-file",
            &connection_file_str,
            "--transport",
            "tcp",
            "--token",
            "none",
        ];

        let mut cmd = create_server_command(&args);
        let child = cmd.spawn().expect("Failed to start kcserver");
        servers.push(child);
        connection_files.push(connection_file_path);

        // Small delay between starts
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Wait for all connection files and read them
    let mut ports = Vec::new();
    for (i, connection_file_path) in connection_files.iter().enumerate() {
        wait_for_connection_file(connection_file_path, 100)
            .await
            .unwrap_or_else(|_| panic!("Connection file {} was not created within timeout", i));

        let connection_content =
            std::fs::read_to_string(connection_file_path).expect("Failed to read connection file");

        #[derive(serde::Deserialize)]
        struct SimpleConnectionInfo {
            port: u16,
        }

        let connection_info: SimpleConnectionInfo =
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
    let (connection_info, child) = test_connection_file_startup(&[], "socket_default", "socket").await;

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

    // Clean up (this will remove the socket file)
    cleanup_spawned_server(child);
    let _ = std::fs::remove_file(&socket_path);

    println!("Successfully tested default socket transport with connection file");
}

#[tokio::test]
async fn test_server_connection_file_explicit_tcp_transport() {
    let (connection_info, child) =
        test_connection_file_startup(&["--transport", "tcp"], "tcp_explicit", "tcp").await;

    assert_eq!(connection_info.transport, "tcp");
    assert!(connection_info.port.is_some());
    assert!(connection_info.base_path.is_some());
    assert!(connection_info.socket_path.is_none());
    assert!(connection_info.named_pipe.is_none());

    let port = connection_info.port.unwrap();
    let base_path = connection_info.base_path.unwrap();

    // Verify the base path is correctly formatted
    assert_eq!(base_path, format!("http://127.0.0.1:{}", port));

    // Clean up
    cleanup_spawned_server(child);

    println!("Successfully tested explicit TCP transport with connection file");
}

#[tokio::test]
#[cfg(unix)]
async fn test_server_connection_file_explicit_socket_transport() {
    let (connection_info, child) = test_connection_file_startup(
        &["--transport", "socket"],
        "socket_explicit",
        "socket",
    )
    .await;

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

    // Clean up (this will remove the socket file)
    cleanup_spawned_server(child);
    let _ = std::fs::remove_file(&socket_path);

    println!("Successfully tested explicit socket transport with connection file");
}

#[tokio::test]
async fn test_invalid_transport_parameter() {
    let temp_dir = std::env::temp_dir();
    let connection_file_path =
        temp_dir.join(format!("kallichore_test_invalid_{}.json", Uuid::new_v4()));
    let connection_file_str = connection_file_path.to_string_lossy().to_string();

    let args = [
        "--connection-file",
        &connection_file_str,
        "--transport",
        "invalid-transport",
        "--token",
        "none",
    ];

    let mut cmd = create_server_command(&args);
    let output = cmd.output().expect("Failed to run command");

    // Server should exit with error code
    assert!(!output.status.success());

    // Should not create connection file
    assert!(!connection_file_path.exists());

    // Clean up (just in case)
    let _ = std::fs::remove_file(&connection_file_path);

    println!("Successfully tested invalid transport parameter rejection");
}
