//
// integration_test.rs
//
// Copyright (C) 2025 Posit Software, PBC. All rights reserved.
// Licensed under the Elastic License 2.0. See LICENSE.txt for license information.
//
//

//! Integration tests for Kallichore server
//!
//! ## Running Tests
//!
//! - Run normally: `cargo test --package kcserver --test integration_test`
//! - Run sequentially: `cargo test --package kcserver --test integration_test -- --test-threads=1`

#![allow(missing_docs)]

#[path = "common/mod.rs"]
mod common;

// Unix socket specific tests remain here since they have special setup
#[cfg(unix)]
mod unix_socket_tests {
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
            // Wait for the socket file to be created
            for _attempt in 0..100 {
                if self.socket_path.exists() {
                    // Try to connect to verify the server is ready
                    if UnixStream::connect(&self.socket_path).is_ok() {
                        println!("Unix socket server ready");
                        return;
                    }
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }

            panic!("Unix socket server failed to start within timeout");
        }

        async fn test_http_status(&self) -> Result<String, Box<dyn std::error::Error>> {
            let mut stream = UnixStream::connect(&self.socket_path)?;

            let request = "GET /status HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
            stream.write_all(request.as_bytes())?;

            let mut response = String::new();
            stream.read_to_string(&mut response)?;

            Ok(response)
        }

        pub fn socket_path(&self) -> &std::path::Path {
            &self.socket_path
        }
    }

    impl Drop for UnixSocketTestServer {
        fn drop(&mut self) {
            println!("Cleaning up Unix socket test server");

            if let Err(e) = self.child.kill() {
                println!("Warning: Failed to terminate Unix socket server: {}", e);
            }

            if let Err(e) = self.child.wait() {
                println!("Warning: Failed to wait for Unix socket server: {}", e);
            }

            // Clean up socket file if it still exists
            if self.socket_path.exists() {
                if let Err(e) = std::fs::remove_file(&self.socket_path) {
                    println!("Warning: Failed to remove socket file: {}", e);
                }
            }
        }
    }

    #[tokio::test]
    async fn test_unix_socket_server_starts_and_responds() {
        let server = UnixSocketTestServer::start().await;

        // Test HTTP status endpoint
        let response = server
            .test_http_status()
            .await
            .expect("Failed to get HTTP status from Unix socket");

        println!("Unix socket HTTP response: {}", response);

        // Should contain HTTP/1.1 200 OK
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.contains("application/json"));

        println!("Unix socket server startup test successful!");
    }

    #[tokio::test]
    async fn test_unix_socket_creates_socket_file() {
        let server = UnixSocketTestServer::start().await;

        // Verify the socket file exists
        assert!(
            server.socket_path().exists(),
            "Socket file should exist at: {:?}",
            server.socket_path()
        );

        // Verify we can connect to it
        let _stream = UnixStream::connect(server.socket_path())
            .expect("Should be able to connect to the Unix socket");

        println!("Unix socket file creation test successful!");
    }

    #[tokio::test]
    async fn test_unix_socket_basic_communication() {
        use crate::common::test_utils::{get_python_executable, is_ipykernel_available};

        // Check if Python and ipykernel are available
        if !is_ipykernel_available().await {
            println!("Skipping Unix socket communication test: Python or ipykernel not available");
            return;
        }

        let _python_cmd = if let Some(cmd) = get_python_executable().await {
            cmd
        } else {
            println!("Skipping Unix socket communication test: No Python executable found");
            return;
        };

        let server = UnixSocketTestServer::start().await;

        // Test basic HTTP status endpoint over Unix socket
        let response = server
            .test_http_status()
            .await
            .expect("Failed to get HTTP status from Unix socket");

        println!("Unix socket HTTP response: {}", response);

        // Should contain HTTP/1.1 200 OK
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.contains("application/json"));

        println!("Unix socket basic communication test successful!");
        // Note: More comprehensive session and WebSocket tests are in python_kernel_tests.rs
    }
}
