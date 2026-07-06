//
// handshake_tests.rs
//
// Copyright (C) 2026 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//

//! Tests for server startup with the client-owned handshake socket

#[path = "common/mod.rs"]
mod common;

use common::test_utils::{
    cleanup_spawned_server, create_server_command, HandshakeConnectionInfo, HandshakeListener,
};
use kallichore_api::{ApiNoContext, Client, ContextWrapperExt, ServerStatusResponse};
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

/// Create a client for the given base path
async fn create_client_for_base_path(
    base_path: &str,
    bearer_token: Option<&str>,
) -> Box<dyn ApiNoContext<ClientContext> + Send + Sync> {
    let context: ClientContext = if let Some(token) = bearer_token {
        swagger::make_context!(
            ContextBuilder,
            EmptyContext,
            Some(AuthData::Bearer(token.to_string())),
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

/// Launch a server that reports over a handshake socket, and return the parsed
/// connection info plus the child process (caller cleans up).
async fn test_handshake_startup(
    extra_args: &[&str],
    expected_transport: &str,
) -> (HandshakeConnectionInfo, Child) {
    // The client (test) creates and listens on the handshake socket first.
    let handshake = HandshakeListener::create().await;
    let handshake_path = handshake.path().to_string();

    let mut args = vec![
        "--port",
        "0",
        "--handshake-socket",
        &handshake_path,
        "--token",
        "none",
    ];
    args.extend_from_slice(extra_args);

    let mut cmd = create_server_command(&args);
    let mut child = cmd.spawn().expect("Failed to start kcserver");

    // Await the single JSON payload from the server.
    let connection_info = match handshake.recv().await {
        Ok(info) => info,
        Err(e) => {
            // Surface any server output to aid debugging.
            let _ = child.kill();
            if let Ok(output) = child.wait_with_output() {
                if !output.stdout.is_empty() {
                    println!("Server stdout: {}", String::from_utf8_lossy(&output.stdout));
                }
                if !output.stderr.is_empty() {
                    println!("Server stderr: {}", String::from_utf8_lossy(&output.stderr));
                }
            }
            panic!("{}", e);
        }
    };

    // Verify the connection info
    assert_eq!(connection_info.transport, expected_transport);
    assert!(connection_info.server_pid > 0, "PID should be greater than 0");
    assert!(
        !connection_info.server_id.is_empty(),
        "server_id should be present"
    );

    (connection_info, child)
}

#[tokio::test]
async fn test_server_starts_with_handshake() {
    let (connection_info, child) = test_handshake_startup(&["--transport", "tcp"], "tcp").await;

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
    let base_path = connection_info.base_path.clone().unwrap();

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
        "Successfully tested server with handshake socket. Port: {}",
        port
    );

    // Clean up
    cleanup_spawned_server(child);
}

#[tokio::test]
async fn test_server_handshake_with_auth_token() {
    let temp_dir = std::env::temp_dir();

    // Create a temporary token file
    let token_file_path = temp_dir.join(format!("kallichore_test_token_{}.txt", Uuid::new_v4()));
    let token_file_str = token_file_path.to_string_lossy().to_string();
    let test_token = "test_auth_token_12345";
    std::fs::write(&token_file_path, test_token).expect("Failed to write token file");

    let handshake = HandshakeListener::create().await;
    let handshake_path = handshake.path().to_string();

    let args = [
        "--port",
        "0",
        "--handshake-socket",
        &handshake_path,
        "--transport",
        "tcp",
        "--token",
        &token_file_str,
    ];

    let mut cmd = create_server_command(&args);
    let child = cmd.spawn().expect("Failed to start kcserver");

    let connection_info = handshake
        .recv()
        .await
        .expect("Failed to receive handshake payload");

    // Verify the connection info includes the auth token
    assert!(connection_info.port.is_some());
    assert_eq!(connection_info.bearer_token, Some(test_token.to_string()));

    let base_path = connection_info.base_path.clone().unwrap();

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
    let _ = std::fs::remove_file(&token_file_path);

    println!(
        "Successfully tested server with handshake socket and auth token. Port: {}",
        connection_info.port.unwrap()
    );
}

#[tokio::test]
async fn test_multiple_servers_different_ports_and_ids() {
    let mut servers = Vec::new();
    let mut ports = Vec::new();
    let mut server_ids = Vec::new();

    // Start 3 servers, each with its own handshake socket.
    for _ in 0..3 {
        let handshake = HandshakeListener::create().await;
        let handshake_path = handshake.path().to_string();

        let args = [
            "--port",
            "0",
            "--handshake-socket",
            &handshake_path,
            "--transport",
            "tcp",
            "--token",
            "none",
        ];

        let mut cmd = create_server_command(&args);
        let child = cmd.spawn().expect("Failed to start kcserver");

        let connection_info = handshake
            .recv()
            .await
            .expect("Failed to receive handshake payload");

        ports.push(connection_info.port.expect("Port should be present"));
        server_ids.push(connection_info.server_id.clone());
        servers.push(child);
    }

    // Verify all servers got different ports
    assert_eq!(ports.len(), 3);
    let mut sorted_ports = ports.clone();
    sorted_ports.sort();
    sorted_ports.dedup();
    assert_eq!(
        sorted_ports.len(),
        3,
        "All servers should have different ports"
    );
    for port in &ports {
        assert!(*port > 0, "Port should be greater than 0");
    }

    // Verify all servers got different server_ids
    let mut sorted_ids = server_ids.clone();
    sorted_ids.sort();
    sorted_ids.dedup();
    assert_eq!(
        sorted_ids.len(),
        3,
        "All servers should have distinct server_ids"
    );

    // Clean up
    for server in servers {
        cleanup_spawned_server(server);
    }

    println!(
        "Successfully tested multiple servers with different ports: {:?} and ids: {:?}",
        ports, server_ids
    );
}

#[tokio::test]
async fn test_server_handshake_default_tcp() {
    // With no --transport (and no --unix-socket), the server defaults to TCP.
    let (connection_info, child) = test_handshake_startup(&[], "tcp").await;

    assert_eq!(connection_info.transport, "tcp");
    assert!(connection_info.port.is_some());
    assert!(connection_info.base_path.is_some());
    assert!(connection_info.socket_path.is_none());
    assert!(connection_info.named_pipe.is_none());

    cleanup_spawned_server(child);

    println!("Successfully tested default TCP transport with handshake socket");
}

#[tokio::test]
async fn test_server_handshake_explicit_tcp_transport() {
    let (connection_info, child) = test_handshake_startup(&["--transport", "tcp"], "tcp").await;

    assert_eq!(connection_info.transport, "tcp");
    assert!(connection_info.port.is_some());
    assert!(connection_info.base_path.is_some());
    assert!(connection_info.socket_path.is_none());
    assert!(connection_info.named_pipe.is_none());

    let port = connection_info.port.unwrap();
    let base_path = connection_info.base_path.clone().unwrap();

    // Verify the base path is correctly formatted
    assert_eq!(base_path, format!("http://127.0.0.1:{}", port));

    // Clean up
    cleanup_spawned_server(child);

    println!("Successfully tested explicit TCP transport with handshake socket");
}

#[tokio::test]
#[cfg(unix)]
async fn test_server_handshake_explicit_socket_transport() {
    let (connection_info, child) =
        test_handshake_startup(&["--transport", "socket"], "socket").await;

    assert_eq!(connection_info.transport, "socket");
    assert!(connection_info.socket_path.is_some());
    assert!(connection_info.port.is_none());
    assert!(connection_info.base_path.is_none());
    assert!(connection_info.named_pipe.is_none());

    let socket_path = connection_info.socket_path.clone().unwrap();

    // Verify the socket file exists
    assert!(
        std::path::Path::new(&socket_path).exists(),
        "Socket file should exist at: {}",
        socket_path
    );

    // Verify the socket file is owner-only (0600)
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&socket_path)
            .expect("Failed to stat socket file")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "Socket file should have 0600 permissions");
    }

    // Test that we can connect to the socket
    use std::os::unix::net::UnixStream;
    let _stream =
        UnixStream::connect(&socket_path).expect("Should be able to connect to the Unix socket");

    // Clean up (this will remove the socket file)
    cleanup_spawned_server(child);
    let _ = std::fs::remove_file(&socket_path);

    println!("Successfully tested explicit socket transport with handshake socket");
}

#[tokio::test]
async fn test_invalid_transport_parameter() {
    let handshake = HandshakeListener::create().await;
    let handshake_path = handshake.path().to_string();

    let args = [
        "--handshake-socket",
        &handshake_path,
        "--transport",
        "invalid-transport",
        "--token",
        "none",
    ];

    let mut cmd = create_server_command(&args);
    let output = cmd.output().expect("Failed to run command");

    // Server should exit with error code (validation fails before the handshake)
    assert!(!output.status.success());

    println!("Successfully tested invalid transport parameter rejection");
}
